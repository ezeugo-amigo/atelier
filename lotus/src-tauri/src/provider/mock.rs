//! Mock providers. They keep the typed-email login form and seed a small
//! mailbox, which makes the UI and the sync engine testable with no network.
//!
//! Seed timestamps are fixed literals, not offsets from `now`. A clock call here
//! would make the assertions in `lib.rs` tests non-deterministic, and no
//! production code path needs an injectable clock: real message times come from
//! the provider's own `internalDate`.

use crate::model::*;
use crate::provider::{BoxFuture, MailProvider};

/// Fixed reference instants for mock seed data.
const SEED_NOW: &str = "2026-07-01T12:10:00Z";
const SEED_EARLIER_TODAY: &str = "2026-07-01T08:10:00Z";
const SEED_YESTERDAY: &str = "2026-06-30T16:45:00Z";
const SEED_SYNCED: &str = "2026-07-01T12:30:00Z";

pub struct MockMailProvider {
    kind: ProviderKind,
    display_name: &'static str,
    description: &'static str,
    account_label: &'static str,
    domain: &'static str,
    accent: &'static str,
}

impl MockMailProvider {
    pub fn gmail() -> Self {
        Self {
            kind: ProviderKind::MockGmail,
            display_name: "Mock Gmail",
            description: "Offline sample mailbox with Gmail-shaped data",
            account_label: "Gmail",
            domain: "gmail.test",
            accent: "#c24135",
        }
    }

    pub fn outlook() -> Self {
        Self {
            kind: ProviderKind::MockOutlook,
            display_name: "Mock Outlook",
            description: "Offline sample mailbox with Outlook-shaped data",
            account_label: "Outlook",
            domain: "outlook.test",
            accent: "#2f5f9f",
        }
    }

    fn seed(&self, login_state: &str, email_address: &str) -> Result<AccountSeed, String> {
        let email = normalize_email(email_address, self.domain)?;
        let account_id = format!("{}-{}", self.kind.slug(), account_slug(&email));
        let inbox_id = format!("{account_id}-inbox");
        let starred_id = format!("{account_id}-starred");
        let drafts_id = format!("{account_id}-drafts");
        let sent_id = format!("{account_id}-sent");
        let archive_id = format!("{account_id}-archive");

        let account = Account {
            id: account_id.clone(),
            display_name: format!("{} Mail", self.account_label),
            email_address: email.clone(),
            provider: self.display_name.into(),
            provider_kind: self.kind,
            accent: self.accent.into(),
            connected: true,
        };

        let folders = vec![
            folder(&inbox_id, &account_id, "Inbox", "inbox"),
            folder(&starred_id, &account_id, "Starred", "starred"),
            folder(&drafts_id, &account_id, "Drafts", "drafts"),
            folder(&sent_id, &account_id, "Sent", "sent"),
            folder(&archive_id, &account_id, "Archive", "archive"),
        ];

        let messages = vec![
            mock_message(
                &format!("{account_id}-welcome"),
                &account_id,
                vec![inbox_id.clone(), starred_id.clone()],
                "Lotus Account Setup",
                "setup@lotus.local",
                &format!("{} connected to Lotus", self.display_name),
                SEED_NOW,
                true,
                true,
                vec![self.account_label, "Setup"],
                vec![email.as_str()],
                vec![
                    format!("The mock authorization callback returned an access token and refresh token for {email}."),
                    "Lotus stored the credential locally and imported this initial mailbox snapshot.".into(),
                ],
            ),
            mock_message(
                &format!("{account_id}-digest"),
                &account_id,
                vec![inbox_id.clone()],
                "Product Updates",
                &format!("updates@{}", self.domain),
                "Your morning digest",
                SEED_EARLIER_TODAY,
                true,
                false,
                vec![self.account_label],
                vec![email.as_str()],
                vec![
                    "Three threads are waiting in the mock mailbox. This message exercises unread counts, snippets, and detail rendering.".into(),
                    "Provider sync code replaces this seed data without changing the wire contract.".into(),
                ],
            ),
            mock_message(
                &format!("{account_id}-receipt"),
                &account_id,
                vec![archive_id.clone()],
                "Billing",
                &format!("billing@{}", self.domain),
                "Receipt for your workspace",
                SEED_YESTERDAY,
                false,
                false,
                vec!["Receipts"],
                vec![email.as_str()],
                vec![
                    "This archived message verifies that provider imports can seed folders beyond the inbox.".into(),
                    "A real provider produces the same normalized shape after fetching remote data.".into(),
                ],
            ),
        ];

        let credential = StoredCredential {
            account_id: account_id.clone(),
            provider: self.kind,
            access_token: format!("mock-access-{}-{login_state}", self.kind.slug()),
            refresh_token: format!("mock-refresh-{}-{login_state}", self.kind.slug()),
            expires_at: "2026-07-01T13:10:00Z".into(),
        };

        let sync_state = ProviderSyncState {
            account_id: account_id.clone(),
            provider: self.kind,
            remote_account_id: email,
            sync_cursor: Some(format!("{}-cursor-1", self.kind.slug())),
            history_id: Some(format!("{}-history-1", self.kind.slug())),
            delta_token: Some(format!("{}-delta-1", self.kind.slug())),
            page_token: None,
            high_watermark: Some(SEED_NOW.into()),
            status: "idle".into(),
            last_attempted_at: Some(SEED_NOW.into()),
            last_successful_at: Some(SEED_NOW.into()),
            last_error: None,
        };

        Ok(AccountSeed {
            account,
            folders,
            messages,
            credential,
            sync_state,
        })
    }
}

impl MailProvider for MockMailProvider {
    fn option(&self) -> ProviderOption {
        ProviderOption {
            provider: self.kind,
            display_name: self.display_name.into(),
            description: self.description.into(),
            browser_login: false,
        }
    }

    fn begin_login(&self) -> BoxFuture<'_, Result<AccountLogin, String>> {
        Box::pin(async move {
            Ok(AccountLogin {
                provider: self.kind,
                login_url: format!("lotus://mock-auth/{}", self.kind.slug()),
                login_state: format!("{}-oauth-state", self.kind.slug()),
                expires_at: "2026-07-01T12:20:00Z".into(),
                scopes: vec![
                    "openid".into(),
                    "email".into(),
                    "mail.read".into(),
                    "offline_access".into(),
                ],
                browser_login: false,
            })
        })
    }

    fn complete_login<'a>(
        &'a self,
        login_state: &'a str,
        email_address: &'a str,
    ) -> BoxFuture<'a, Result<AccountSeed, String>> {
        Box::pin(async move { self.seed(login_state, email_address) })
    }

    fn sync_mailbox<'a>(
        &'a self,
        state: &'a ProviderSyncState,
    ) -> BoxFuture<'a, Result<MailboxDelta, String>> {
        Box::pin(async move {
            let account_id = state.account_id.clone();
            let inbox_id = format!("{account_id}-inbox");
            let mut next_state = state.clone();
            next_state.status = "idle".into();
            next_state.last_attempted_at = Some(now_iso8601());
            next_state.last_successful_at = Some(now_iso8601());
            next_state.last_error = None;

            let first_cursor = format!("{}-cursor-1", self.kind.slug());
            let messages = if state.sync_cursor.as_deref() == Some(first_cursor.as_str()) {
                next_state.sync_cursor = Some(format!("{}-cursor-2", self.kind.slug()));
                next_state.history_id = Some(format!("{}-history-2", self.kind.slug()));
                next_state.delta_token = Some(format!("{}-delta-2", self.kind.slug()));
                next_state.high_watermark = Some(SEED_SYNCED.into());

                vec![mock_message(
                    &format!("{account_id}-provider-sync-2"),
                    &account_id,
                    vec![inbox_id],
                    "Inbox Sync",
                    &format!("sync@{}", self.domain),
                    "New message cached for offline reading",
                    SEED_SYNCED,
                    true,
                    false,
                    vec![self.account_label, "Synced"],
                    vec![state.remote_account_id.as_str()],
                    vec![
                        "This message arrived through the provider sync path, not through account setup.".into(),
                        "Because it lives in the local mailbox cache, Lotus renders it while offline.".into(),
                    ],
                )]
            } else {
                Vec::new()
            };

            Ok(MailboxDelta {
                folders: Vec::new(),
                messages,
                sync_state: next_state,
            })
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[allow(clippy::too_many_arguments)]
fn mock_message(
    id: &str,
    account_id: &str,
    folder_ids: Vec<String>,
    sender_name: &str,
    sender_email: &str,
    subject: &str,
    received_at: &str,
    unread: bool,
    starred: bool,
    labels: Vec<&str>,
    to: Vec<&str>,
    body_paragraphs: Vec<String>,
) -> MessageDetail {
    MessageDetail {
        id: id.into(),
        account_id: account_id.into(),
        folder_ids,
        sender_name: sender_name.into(),
        sender_email: sender_email.into(),
        subject: subject.into(),
        snippet: derive_snippet(&body_paragraphs.join(" ")),
        received_at: received_at.into(),
        internal_date: iso8601_to_epoch_millis(received_at),
        unread,
        starred,
        labels: labels.into_iter().map(String::from).collect(),
        to: to.into_iter().map(String::from).collect(),
        cc: Vec::new(),
        reply_to: Vec::new(),
        body_paragraphs,
        provider_message_id: None,
        provider_thread_id: None,
    }
}

fn normalize_email(email_address: &str, fallback_domain: &str) -> Result<String, String> {
    let trimmed = email_address.trim().to_lowercase();
    if trimmed.is_empty() {
        return Err("Enter an email address to continue".into());
    }

    if trimmed.contains('@') {
        Ok(trimmed)
    } else {
        Ok(format!("{trimmed}@{fallback_domain}"))
    }
}

fn account_slug(email_address: &str) -> String {
    email_address
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect()
}
