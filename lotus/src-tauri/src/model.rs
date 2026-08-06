//! Wire and domain types shared by the storage, provider, and sync layers.
//!
//! Two conventions matter here and are load-bearing across all four layers.
//! Timestamps are ISO-8601 UTC strings on the wire and in SQLite, never display
//! prose, so the sort key and the rendered value cannot diverge. A message
//! belongs to many folders at once (`folder_ids`), because a Gmail message
//! carries `INBOX` and `CATEGORY_PROMOTIONS` and user labels simultaneously.

use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub const UNIFIED_INBOX_ID: &str = "unified-inbox";

/// Attempts before an outbox item stops being retried and parks as `failed`.
pub const OUTBOX_ATTEMPT_CAP: i64 = 5;

/// When to try a failed outbox item again: 30s, 1m, 2m, 4m. Bounded so a long
/// offline stretch does not push the next attempt hours out.
pub fn retry_at(attempts: i64) -> String {
    let backoff_seconds = 30i64.saturating_mul(1 << attempts.clamp(0, 6)).min(15 * 60);
    let deadline = OffsetDateTime::now_utc() + time::Duration::seconds(backoff_seconds);
    deadline
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

/// Persisted provider discriminant. Distinct from `Account.provider`, which is
/// a display label. The schema's `UNIQUE (provider, provider_account_id)` needs
/// this stable machine value.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    Gmail,
    MockGmail,
    MockOutlook,
}

impl ProviderKind {
    pub fn slug(self) -> &'static str {
        match self {
            ProviderKind::Gmail => "gmail",
            ProviderKind::MockGmail => "mock-gmail",
            ProviderKind::MockOutlook => "mock-outlook",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            "gmail" => Some(ProviderKind::Gmail),
            "mock-gmail" => Some(ProviderKind::MockGmail),
            "mock-outlook" => Some(ProviderKind::MockOutlook),
            _ => None,
        }
    }

    /// True when the flow opens a real system browser instead of the mock form.
    pub fn uses_browser_login(self) -> bool {
        matches!(self, ProviderKind::Gmail)
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOption {
    pub provider: ProviderKind,
    pub display_name: String,
    pub description: String,
    /// Elm branches on this to pick the browser flow over the mock email form.
    pub browser_login: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub display_name: String,
    pub email_address: String,
    /// Display label, for example "Gmail". Not a discriminant.
    pub provider: String,
    pub provider_kind: ProviderKind,
    pub accent: String,
    pub connected: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub role: String,
    /// Provider-side identifier. For Gmail this is the label id, e.g. `INBOX`.
    pub provider_folder_id: Option<String>,
    pub unread_count: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSummary {
    pub id: String,
    pub account_id: String,
    pub folder_ids: Vec<String>,
    pub sender_name: String,
    pub sender_email: String,
    pub subject: String,
    pub snippet: String,
    /// ISO-8601 UTC. Elm parses and formats for display.
    pub received_at: String,
    pub unread: bool,
    pub starred: bool,
    pub labels: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetail {
    pub id: String,
    pub account_id: String,
    pub folder_ids: Vec<String>,
    pub sender_name: String,
    pub sender_email: String,
    pub subject: String,
    pub snippet: String,
    pub received_at: String,
    /// Epoch millis, from Gmail's `internalDate`. The SQLite sort key.
    pub internal_date: i64,
    pub unread: bool,
    pub starred: bool,
    pub labels: Vec<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub reply_to: Vec<String>,
    pub body_paragraphs: Vec<String>,
    pub provider_message_id: Option<String>,
    pub provider_thread_id: Option<String>,
}

impl MessageDetail {
    pub fn summary(&self) -> MessageSummary {
        MessageSummary {
            id: self.id.clone(),
            account_id: self.account_id.clone(),
            folder_ids: self.folder_ids.clone(),
            sender_name: self.sender_name.clone(),
            sender_email: self.sender_email.clone(),
            subject: self.subject.clone(),
            snippet: self.snippet.clone(),
            received_at: self.received_at.clone(),
            unread: self.unread,
            starred: self.starred,
            labels: self.labels.clone(),
        }
    }

    #[allow(dead_code)] // used by the test-only in-memory store
    pub fn in_folder(&self, folder_id: &str) -> bool {
        self.folder_ids.iter().any(|id| id == folder_id)
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    /// Display-only prose, unlike `last_checked`.
    pub state: String,
    /// ISO-8601 UTC, or `None` when no sync has run.
    pub last_checked: Option<String>,
    pub detail: String,
}

/// A credential as held by a provider immediately after login. Refresh tokens
/// live in the OS keychain; this struct is the hand-off, never a storage row.
#[derive(Clone)]
pub struct StoredCredential {
    pub account_id: String,
    pub provider: ProviderKind,
    pub access_token: String,
    pub refresh_token: String,
    /// ISO-8601 UTC.
    pub expires_at: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialPreview {
    pub account_id: String,
    pub provider: ProviderKind,
    pub access_token_tail: String,
    pub refresh_token_tail: String,
    pub expires_at: String,
}

impl StoredCredential {
    pub fn preview(&self) -> CredentialPreview {
        CredentialPreview {
            account_id: self.account_id.clone(),
            provider: self.provider,
            access_token_tail: token_tail(&self.access_token),
            refresh_token_tail: token_tail(&self.refresh_token),
            expires_at: self.expires_at.clone(),
        }
    }
}

#[derive(Clone)]
pub struct ProviderSyncState {
    pub account_id: String,
    pub provider: ProviderKind,
    pub remote_account_id: String,
    pub sync_cursor: Option<String>,
    pub history_id: Option<String>,
    pub delta_token: Option<String>,
    pub page_token: Option<String>,
    pub high_watermark: Option<String>,
    pub status: String,
    pub last_attempted_at: Option<String>,
    pub last_successful_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSyncCheckpoint {
    pub account_id: String,
    pub provider: ProviderKind,
    pub remote_account_id: String,
    pub sync_cursor: Option<String>,
    pub history_id: Option<String>,
    pub delta_token: Option<String>,
    pub page_token: Option<String>,
    pub high_watermark: Option<String>,
    pub status: String,
    pub last_attempted_at: Option<String>,
    pub last_successful_at: Option<String>,
    pub last_error: Option<String>,
}

impl From<ProviderSyncState> for ProviderSyncCheckpoint {
    fn from(state: ProviderSyncState) -> Self {
        Self {
            account_id: state.account_id,
            provider: state.provider,
            remote_account_id: state.remote_account_id,
            sync_cursor: state.sync_cursor,
            history_id: state.history_id,
            delta_token: state.delta_token,
            page_token: state.page_token,
            high_watermark: state.high_watermark,
            status: state.status,
            last_attempted_at: state.last_attempted_at,
            last_successful_at: state.last_successful_at,
            last_error: state.last_error,
        }
    }
}

#[derive(Clone)]
pub struct NewOutboxItem {
    pub account_id: String,
    pub provider: ProviderKind,
    pub operation: String,
    pub payload_json: String,
}

#[derive(Clone)]
pub struct SyncOutboxItem {
    pub id: String,
    pub account_id: String,
    pub provider: ProviderKind,
    pub operation: String,
    pub payload_json: String,
    /// One of `pending`, `retryable`, `synced`, `failed`.
    pub status: String,
    pub attempt_count: usize,
    pub created_at: String,
    pub last_attempted_at: Option<String>,
    pub last_synced_at: Option<String>,
    pub last_error: Option<String>,
    pub next_attempt_at: Option<String>,
    pub failure_kind: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncOutboxEntry {
    pub id: String,
    pub account_id: String,
    pub provider: ProviderKind,
    pub operation: String,
    pub payload_json: String,
    pub status: String,
    pub attempt_count: usize,
    pub created_at: String,
    pub last_attempted_at: Option<String>,
    pub last_synced_at: Option<String>,
    pub last_error: Option<String>,
    pub next_attempt_at: Option<String>,
    pub failure_kind: Option<String>,
}

impl From<SyncOutboxItem> for SyncOutboxEntry {
    fn from(item: SyncOutboxItem) -> Self {
        Self {
            id: item.id,
            account_id: item.account_id,
            provider: item.provider,
            operation: item.operation,
            payload_json: item.payload_json,
            status: item.status,
            attempt_count: item.attempt_count,
            created_at: item.created_at,
            last_attempted_at: item.last_attempted_at,
            last_synced_at: item.last_synced_at,
            last_error: item.last_error,
            next_attempt_at: item.next_attempt_at,
            failure_kind: item.failure_kind,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncOutboxSummary {
    pub pending: usize,
    pub synced: usize,
    pub failed: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub attempted: usize,
    pub synced: usize,
    pub failed: usize,
    pub inbound_messages: usize,
    pub remaining_pending: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapData {
    pub provider_options: Vec<ProviderOption>,
    pub accounts: Vec<Account>,
    pub folders: Vec<Folder>,
    pub messages: Vec<MessageSummary>,
    pub selected_folder_id: String,
    pub selected_message_id: Option<String>,
    pub selected_message: Option<MessageDetail>,
    pub sync_status: SyncStatus,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxSnapshot {
    pub folder_id: String,
    pub folders: Vec<Folder>,
    pub messages: Vec<MessageSummary>,
    pub selected_message_id: Option<String>,
    pub selected_message: Option<MessageDetail>,
    pub sync_status: SyncStatus,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageUpdate {
    pub folders: Vec<Folder>,
    pub message: MessageDetail,
    pub sync_status: SyncStatus,
}

/// Returned by `begin_account_login`. For browser providers the login lands
/// later as a `lotus://event`; `login_state` is how Elm correlates it.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountLogin {
    pub provider: ProviderKind,
    pub login_url: String,
    pub login_state: String,
    pub expires_at: String,
    pub scopes: Vec<String>,
    pub browser_login: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSetupResult {
    pub bootstrap: BootstrapData,
    pub credential: CredentialPreview,
}

pub struct AccountSeed {
    pub account: Account,
    pub folders: Vec<Folder>,
    pub messages: Vec<MessageDetail>,
    pub credential: StoredCredential,
    pub sync_state: ProviderSyncState,
}

pub struct MailboxDelta {
    pub folders: Vec<Folder>,
    pub messages: Vec<MessageDetail>,
    pub sync_state: ProviderSyncState,
}

/// ISO-8601 UTC for the current instant. The only clock call in the codebase
/// outside of test fixtures: real message times come from the provider.
pub fn now_iso8601() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

pub fn epoch_millis_to_iso8601(millis: i64) -> String {
    OffsetDateTime::from_unix_timestamp_nanos((millis as i128) * 1_000_000)
        .ok()
        .and_then(|value| value.format(&Rfc3339).ok())
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".into())
}

pub fn iso8601_to_epoch_millis(value: &str) -> i64 {
    OffsetDateTime::parse(value, &Rfc3339)
        .map(|parsed| (parsed.unix_timestamp_nanos() / 1_000_000) as i64)
        .unwrap_or(0)
}

pub fn token_tail(token: &str) -> String {
    let mut tail: Vec<char> = token.chars().rev().take(6).collect();
    tail.reverse();
    tail.into_iter().collect()
}

pub fn folder(id: &str, account_id: &str, name: &str, role: &str) -> Folder {
    Folder {
        id: id.into(),
        account_id: account_id.into(),
        name: name.into(),
        role: role.into(),
        provider_folder_id: None,
        unread_count: 0,
    }
}

pub fn failed_sync_state(mut state: ProviderSyncState, error: String) -> ProviderSyncState {
    state.status = "failed".into();
    state.last_attempted_at = Some(now_iso8601());
    state.last_error = Some(error);
    state
}

/// Collapse whitespace and cut on a word boundary. Gmail's own `snippet` is
/// unavailable under `format=RAW`, and a second METADATA fetch would double the
/// quota cost per message.
pub fn derive_snippet(body: &str) -> String {
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= 200 {
        return collapsed;
    }

    let truncated: String = collapsed.chars().take(200).collect();
    match truncated.rfind(' ') {
        Some(index) if index > 80 => format!("{}...", &truncated[..index]),
        _ => format!("{truncated}..."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_round_trip_through_epoch_millis() {
        let iso = epoch_millis_to_iso8601(1_751_378_400_000);
        assert_eq!(iso8601_to_epoch_millis(&iso), 1_751_378_400_000);
    }

    #[test]
    fn snippet_collapses_whitespace_and_cuts_on_word_boundary() {
        assert_eq!(derive_snippet("  one\n\ttwo   three "), "one two three");

        let long = "word ".repeat(80);
        let snippet = derive_snippet(&long);
        assert!(snippet.ends_with("..."));
        assert!(snippet.chars().count() <= 203);
        assert!(!snippet.contains("  "));
    }

    #[test]
    fn provider_slug_round_trips() {
        for kind in [
            ProviderKind::Gmail,
            ProviderKind::MockGmail,
            ProviderKind::MockOutlook,
        ] {
            assert_eq!(ProviderKind::from_slug(kind.slug()), Some(kind));
        }
        assert_eq!(ProviderKind::from_slug("nope"), None);
    }
}
