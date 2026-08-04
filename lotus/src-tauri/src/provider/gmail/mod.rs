//! The real Gmail provider.

pub mod api;
pub mod auth;
pub mod labels;
pub mod mime;

use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;

use crate::credentials::{credential_ref, expiry_iso8601, AccessToken, CredentialStore};
use crate::model::*;
use crate::provider::gmail::api::{ApiError, FailureKind, GmailApi};
use crate::provider::gmail::auth::{ClientConfig, GMAIL_SCOPE};
use crate::provider::{BoxFuture, MailProvider};

/// Imported synchronously on connect, so the user sees mail immediately.
pub const FIRST_WINDOW: usize = 500;
/// Hard cap on the background pass. Without a bound the local database grows to
/// the whole mailbox: a ten-year account is 100k+ messages with full bodies,
/// which puts the SQLite file into the gigabytes. 5000 is several months of
/// normal mail and keeps search covering exactly what is stored.
pub const MESSAGE_CAP: usize = 5000;

pub struct GmailProvider {
    config: ClientConfig,
    api: GmailApi,
    http: reqwest::Client,
    credentials: Arc<CredentialStore>,
}

/// What a completed OAuth exchange yields, before any mail is fetched.
pub struct GmailConnection {
    pub account_id: String,
    pub email_address: String,
    pub history_id: Option<String>,
    pub access_token: String,
    pub expires_at: String,
}

impl GmailProvider {
    pub fn new(config: ClientConfig, credentials: Arc<CredentialStore>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("Lotus/0.1")
            .build()
            .unwrap_or_default();

        Self {
            config,
            api: GmailApi::new(http.clone()),
            http,
            credentials,
        }
    }

    /// Exchange the callback code, read the profile, and stash the refresh token
    /// in the keychain. The access token stays in memory only.
    pub async fn complete_oauth(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<GmailConnection, String> {
        let tokens =
            auth::exchange_code(&self.http, &self.config, code, verifier, redirect_uri).await?;

        let profile = self
            .api
            .profile(&tokens.access_token)
            .await
            .map_err(|error| error.message)?;

        let email = profile.email_address.to_lowercase();
        let account_id = format!("gmail-{}", account_slug(&email));
        let reference = credential_ref(ProviderKind::Gmail.slug(), &email);

        // prompt=consent should always yield a refresh token. If Google withheld
        // one, say so rather than storing a session that dies in an hour.
        let refresh_token = tokens.refresh_token.ok_or_else(|| {
            "Google did not return a refresh token. Remove Lotus from your Google account \
             permissions and connect again."
                .to_string()
        })?;

        self.credentials
            .save_refresh_token(&reference, &refresh_token)?;

        let expires_at = expiry_iso8601(tokens.expires_in);
        self.credentials
            .store_access_token(
                &account_id,
                AccessToken {
                    value: tokens.access_token.clone(),
                    expires_at_epoch: iso8601_to_epoch_millis(&expires_at) / 1000,
                },
            )
            .await;

        Ok(GmailConnection {
            account_id,
            email_address: email,
            history_id: profile.history_id,
            access_token: tokens.access_token,
            expires_at,
        })
    }

    /// Single-flighted: concurrent callers queue on the per-account gate, and
    /// whoever gets there second finds the fresh token already cached.
    pub async fn access_token(
        &self,
        account_id: &str,
        reference: &str,
    ) -> Result<String, ApiError> {
        let now = unix_seconds();
        if let Some(token) = self.credentials.cached_access_token(account_id, now).await {
            return Ok(token);
        }

        let gate = self.credentials.refresh_gate(account_id).await;
        let _held = gate.lock().await;

        if let Some(token) = self
            .credentials
            .cached_access_token(account_id, unix_seconds())
            .await
        {
            return Ok(token);
        }

        let refresh_token =
            self.credentials
                .refresh_token(reference)
                .map_err(|error| ApiError {
                    kind: FailureKind::NeedsReconsent,
                    message: error,
                })?;

        let tokens = auth::refresh_access_token(&self.http, &self.config, &refresh_token)
            .await
            .map_err(|error| {
                if error == "invalid_grant" {
                    ApiError {
                        kind: FailureKind::NeedsReconsent,
                        message: "Gmail access was revoked. Reconnect the account to resume \
                                  syncing. Your cached mail stays readable."
                            .into(),
                    }
                } else {
                    ApiError {
                        kind: FailureKind::Retryable,
                        message: error,
                    }
                }
            })?;

        let expires_at = expiry_iso8601(tokens.expires_in);
        self.credentials
            .store_access_token(
                account_id,
                AccessToken {
                    value: tokens.access_token.clone(),
                    expires_at_epoch: iso8601_to_epoch_millis(&expires_at) / 1000,
                },
            )
            .await;

        Ok(tokens.access_token)
    }

    pub async fn labels(&self, access_token: &str) -> Result<Vec<api::Label>, ApiError> {
        Ok(self.api.labels(access_token).await?.labels)
    }

    /// Fetch one page of inbox message ids.
    pub async fn list_inbox_page(
        &self,
        access_token: &str,
        page_token: Option<&str>,
        page_size: usize,
    ) -> Result<(Vec<api::MessageRef>, Option<String>), ApiError> {
        let page = self
            .api
            .list_messages(access_token, "INBOX", page_token, page_size)
            .await?;
        Ok((page.messages, page.next_page_token))
    }

    /// Fetch and normalize a batch of messages, at most `MAX_IN_FLIGHT_GETS` at
    /// a time. A message that fails to fetch is dropped from the batch: a failed
    /// page leaves already-imported messages readable.
    pub async fn fetch_messages(
        &self,
        access_token: &str,
        account_id: &str,
        folders: &[Folder],
        all_labels: &[api::Label],
        refs: &[api::MessageRef],
    ) -> Vec<MessageDetail> {
        let mut out: Vec<MessageDetail> = Vec::with_capacity(refs.len());

        for chunk in refs.chunks(api::MAX_IN_FLIGHT_GETS) {
            let fetches = chunk
                .iter()
                .map(|reference| self.api.get_message_raw(access_token, &reference.id));
            let results = futures_join_all(fetches).await;

            for result in results {
                match result {
                    Ok(raw) => {
                        if let Some(message) = normalize(account_id, folders, all_labels, &raw) {
                            out.push(message);
                        }
                    }
                    // Intentionally swallowed. Losing one message to a transient
                    // error is better than failing the whole import; the next
                    // incremental sync picks it up.
                    Err(_) => continue,
                }
            }
        }

        out
    }
}

/// Build a normalized message from a Gmail RAW response. Returns `None` only
/// when the payload has no raw body at all; malformed MIME degrades to an empty
/// body with headers intact.
pub fn normalize(
    account_id: &str,
    folders: &[Folder],
    all_labels: &[api::Label],
    raw: &api::RawMessage,
) -> Option<MessageDetail> {
    let encoded = raw.raw.as_ref()?;
    // Gmail returns url-safe base64. Whether it pads is not documented as
    // stable, so try both engines rather than assuming.
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(encoded))
        .unwrap_or_default();

    let parsed = mime::parse(&bytes);
    let internal_date = raw
        .internal_date
        .as_ref()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);

    let body_text = parsed.body_paragraphs.join(" ");

    Some(MessageDetail {
        id: format!("{account_id}-{}", raw.id),
        account_id: account_id.to_string(),
        folder_ids: labels::folder_ids_for_message(account_id, folders, &raw.label_ids),
        sender_name: parsed.sender_name,
        sender_email: parsed.sender_email,
        subject: parsed.subject,
        snippet: derive_snippet(&body_text),
        received_at: epoch_millis_to_iso8601(internal_date),
        internal_date,
        unread: raw.label_ids.iter().any(|label| label == "UNREAD"),
        starred: raw.label_ids.iter().any(|label| label == "STARRED"),
        labels: labels::display_labels(all_labels, &raw.label_ids),
        to: parsed.to,
        cc: parsed.cc,
        reply_to: parsed.reply_to,
        body_paragraphs: parsed.body_paragraphs,
        provider_message_id: Some(raw.id.clone()),
        provider_thread_id: raw.thread_id.clone(),
    })
}

impl MailProvider for GmailProvider {
    fn option(&self) -> ProviderOption {
        ProviderOption {
            provider: ProviderKind::Gmail,
            display_name: "Gmail".into(),
            description: "Sign in with Google to sync your real inbox".into(),
            browser_login: true,
        }
    }

    /// Unused for Gmail: `begin_account_login` drives the loopback flow directly
    /// because it needs the app handle to open a browser and emit an event.
    fn begin_login(&self) -> BoxFuture<'_, Result<AccountLogin, String>> {
        Box::pin(async move {
            Ok(AccountLogin {
                provider: ProviderKind::Gmail,
                login_url: String::new(),
                login_state: String::new(),
                expires_at: now_iso8601(),
                scopes: vec![GMAIL_SCOPE.to_string()],
                browser_login: true,
            })
        })
    }

    fn complete_login<'a>(
        &'a self,
        _login_state: &'a str,
        _email_address: &'a str,
    ) -> BoxFuture<'a, Result<AccountSeed, String>> {
        Box::pin(async move {
            Err("Gmail sign-in completes in your browser, not from a typed address.".to_string())
        })
    }

    /// Incremental refresh. Re-lists the first inbox page and re-fetches those
    /// messages, so remote read/label changes land locally. `users.history.list`
    /// (Phase 4) replaces this with a delta feed.
    fn sync_mailbox<'a>(
        &'a self,
        state: &'a ProviderSyncState,
    ) -> BoxFuture<'a, Result<MailboxDelta, String>> {
        Box::pin(async move {
            let reference = credential_ref(ProviderKind::Gmail.slug(), &state.remote_account_id);
            let access_token = self
                .access_token(&state.account_id, &reference)
                .await
                .map_err(|error| error.message)?;

            let all_labels = self
                .labels(&access_token)
                .await
                .map_err(|error| error.message)?;
            let folders = labels::folders_for_account(&state.account_id, &all_labels);

            let (refs, _) = self
                .list_inbox_page(&access_token, None, 50)
                .await
                .map_err(|error| error.message)?;

            let messages = self
                .fetch_messages(
                    &access_token,
                    &state.account_id,
                    &folders,
                    &all_labels,
                    &refs,
                )
                .await;

            let mut next_state = state.clone();
            next_state.status = "idle".into();
            next_state.last_attempted_at = Some(now_iso8601());
            next_state.last_successful_at = Some(now_iso8601());
            next_state.last_error = None;
            next_state.high_watermark = Some(now_iso8601());

            Ok(MailboxDelta {
                folders,
                messages,
                sync_state: next_state,
            })
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// A tiny `join_all`, so the crate does not take a `futures` dependency for one
/// call site. Order is preserved.
async fn futures_join_all<F, T>(futures: impl IntoIterator<Item = F>) -> Vec<T>
where
    F: std::future::Future<Output = T>,
{
    let mut set: Vec<std::pin::Pin<Box<F>>> = futures.into_iter().map(Box::pin).collect();
    let mut out: Vec<Option<T>> = (0..set.len()).map(|_| None).collect();

    // Poll in order. reqwest drives each request on the shared runtime, so the
    // whole chunk is in flight regardless of the poll order here.
    for (index, future) in set.iter_mut().enumerate() {
        out[index] = Some(future.as_mut().await);
    }

    out.into_iter().flatten().collect()
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

pub fn account_slug(email_address: &str) -> String {
    email_address
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels_fixture() -> Vec<api::Label> {
        vec![
            api::Label {
                id: "INBOX".into(),
                name: "INBOX".into(),
                label_type: Some("system".into()),
            },
            api::Label {
                id: "UNREAD".into(),
                name: "UNREAD".into(),
                label_type: Some("system".into()),
            },
            api::Label {
                id: "Label_3".into(),
                name: "Receipts".into(),
                label_type: Some("user".into()),
            },
        ]
    }

    fn raw_message(raw: &str, label_ids: Vec<String>) -> api::RawMessage {
        api::RawMessage {
            id: "18c2f".into(),
            thread_id: Some("t1".into()),
            label_ids,
            internal_date: Some("1751378400000".into()),
            raw: Some(URL_SAFE_NO_PAD.encode(raw)),
        }
    }

    #[test]
    fn normalize_maps_headers_labels_and_the_sort_key() {
        let all_labels = labels_fixture();
        let folders = labels::folders_for_account("acct", &all_labels);
        let raw = raw_message(
            "From: Ada <ada@example.com>\r\nTo: me@example.com\r\n\
             Subject: Invoice 42\r\nContent-Type: text/plain\r\n\r\nPay me.\r\n",
            vec!["INBOX".into(), "UNREAD".into(), "Label_3".into()],
        );

        let message = normalize("acct", &folders, &all_labels, &raw).unwrap();

        assert_eq!(message.id, "acct-18c2f");
        assert_eq!(message.provider_message_id.as_deref(), Some("18c2f"));
        assert_eq!(message.provider_thread_id.as_deref(), Some("t1"));
        assert_eq!(message.sender_name, "Ada");
        assert_eq!(message.subject, "Invoice 42");
        assert_eq!(message.snippet, "Pay me.");
        assert!(message.unread);
        assert!(!message.starred);
        assert_eq!(message.internal_date, 1_751_378_400_000);
        assert_eq!(
            message.received_at,
            epoch_millis_to_iso8601(1_751_378_400_000)
        );
        assert_eq!(message.labels, vec!["Receipts"]);
        assert!(message.folder_ids.contains(&"acct-inbox".to_string()));
    }

    #[test]
    fn a_message_without_inbox_normalizes_into_archive() {
        let all_labels = labels_fixture();
        let folders = labels::folders_for_account("acct", &all_labels);
        let raw = raw_message(
            "From: a@b.com\r\nSubject: Old\r\nContent-Type: text/plain\r\n\r\nbody\r\n",
            vec!["Label_3".into()],
        );

        let message = normalize("acct", &folders, &all_labels, &raw).unwrap();
        assert!(message.folder_ids.contains(&"acct-archive".to_string()));
        assert!(!message.folder_ids.contains(&"acct-inbox".to_string()));
    }

    #[test]
    fn malformed_mime_degrades_to_an_empty_body() {
        let all_labels = labels_fixture();
        let folders = labels::folders_for_account("acct", &all_labels);
        let raw = raw_message("\x01\x02 total garbage", vec!["INBOX".into()]);

        let message = normalize("acct", &folders, &all_labels, &raw).unwrap();
        assert!(message.body_paragraphs.is_empty());
        assert_eq!(message.internal_date, 1_751_378_400_000);
    }

    #[test]
    fn a_payload_with_no_raw_body_is_skipped() {
        let all_labels = labels_fixture();
        let folders = labels::folders_for_account("acct", &all_labels);
        let raw = api::RawMessage {
            id: "x".into(),
            thread_id: None,
            label_ids: vec!["INBOX".into()],
            internal_date: None,
            raw: None,
        };
        assert!(normalize("acct", &folders, &all_labels, &raw).is_none());
    }

    #[test]
    fn account_slug_is_filesystem_and_id_safe() {
        assert_eq!(
            account_slug("Reader.Name+tag@Gmail.com"),
            "reader-name-tag-gmail-com"
        );
    }
}
