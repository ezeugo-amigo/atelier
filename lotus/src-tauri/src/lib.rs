use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

const UNIFIED_INBOX_ID: &str = "unified-inbox";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum ProviderKind {
    MockGmail,
    MockOutlook,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderOption {
    provider: ProviderKind,
    display_name: String,
    description: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    id: String,
    display_name: String,
    email_address: String,
    provider: String,
    accent: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Folder {
    id: String,
    account_id: String,
    name: String,
    role: String,
    unread_count: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageSummary {
    id: String,
    account_id: String,
    folder_id: String,
    sender_name: String,
    sender_email: String,
    subject: String,
    snippet: String,
    received_at: String,
    unread: bool,
    starred: bool,
    labels: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageDetail {
    id: String,
    account_id: String,
    folder_id: String,
    sender_name: String,
    sender_email: String,
    subject: String,
    snippet: String,
    received_at: String,
    unread: bool,
    starred: bool,
    labels: Vec<String>,
    to: Vec<String>,
    cc: Vec<String>,
    body_paragraphs: Vec<String>,
}

impl MessageDetail {
    fn summary(&self) -> MessageSummary {
        MessageSummary {
            id: self.id.clone(),
            account_id: self.account_id.clone(),
            folder_id: self.folder_id.clone(),
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
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncStatus {
    state: String,
    last_checked: String,
    detail: String,
}

#[derive(Clone)]
struct StoredCredential {
    account_id: String,
    provider: ProviderKind,
    access_token: String,
    refresh_token: String,
    expires_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CredentialPreview {
    account_id: String,
    provider: ProviderKind,
    access_token_tail: String,
    refresh_token_tail: String,
    expires_at: String,
}

impl StoredCredential {
    fn preview(&self) -> CredentialPreview {
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
struct ProviderSyncState {
    account_id: String,
    provider: ProviderKind,
    remote_account_id: String,
    sync_cursor: Option<String>,
    history_id: Option<String>,
    delta_token: Option<String>,
    page_token: Option<String>,
    high_watermark: Option<String>,
    status: String,
    last_attempted_at: Option<String>,
    last_successful_at: Option<String>,
    last_error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderSyncCheckpoint {
    account_id: String,
    provider: ProviderKind,
    remote_account_id: String,
    sync_cursor: Option<String>,
    history_id: Option<String>,
    delta_token: Option<String>,
    page_token: Option<String>,
    high_watermark: Option<String>,
    status: String,
    last_attempted_at: Option<String>,
    last_successful_at: Option<String>,
    last_error: Option<String>,
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
struct NewOutboxItem {
    account_id: String,
    provider: ProviderKind,
    operation: String,
    payload_json: String,
}

#[derive(Clone)]
struct SyncOutboxItem {
    id: String,
    account_id: String,
    provider: ProviderKind,
    operation: String,
    payload_json: String,
    status: String,
    attempt_count: usize,
    created_at: String,
    last_attempted_at: Option<String>,
    last_synced_at: Option<String>,
    last_error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncOutboxEntry {
    id: String,
    account_id: String,
    provider: ProviderKind,
    operation: String,
    payload_json: String,
    status: String,
    attempt_count: usize,
    created_at: String,
    last_attempted_at: Option<String>,
    last_synced_at: Option<String>,
    last_error: Option<String>,
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
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncOutboxSummary {
    pending: usize,
    synced: usize,
    failed: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncReport {
    attempted: usize,
    synced: usize,
    failed: usize,
    inbound_messages: usize,
    remaining_pending: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapData {
    provider_options: Vec<ProviderOption>,
    accounts: Vec<Account>,
    folders: Vec<Folder>,
    messages: Vec<MessageSummary>,
    selected_folder_id: String,
    selected_message_id: Option<String>,
    selected_message: Option<MessageDetail>,
    sync_status: SyncStatus,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MailboxSnapshot {
    folder_id: String,
    folders: Vec<Folder>,
    messages: Vec<MessageSummary>,
    selected_message_id: Option<String>,
    selected_message: Option<MessageDetail>,
    sync_status: SyncStatus,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageUpdate {
    folders: Vec<Folder>,
    message: MessageDetail,
    sync_status: SyncStatus,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountLogin {
    provider: ProviderKind,
    login_url: String,
    login_state: String,
    expires_at: String,
    scopes: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountSetupResult {
    bootstrap: BootstrapData,
    credential: CredentialPreview,
}

struct AccountSeed {
    account: Account,
    folders: Vec<Folder>,
    messages: Vec<MessageDetail>,
    credential: StoredCredential,
    sync_state: ProviderSyncState,
}

struct MailboxDelta {
    folders: Vec<Folder>,
    messages: Vec<MessageDetail>,
    sync_state: ProviderSyncState,
}

struct MailStore {
    accounts: Vec<Account>,
    folders: Vec<Folder>,
    messages: Vec<MessageDetail>,
    credentials: Vec<StoredCredential>,
    sync_states: Vec<ProviderSyncState>,
    outbox: Vec<SyncOutboxItem>,
    next_outbox_id: usize,
    selected_folder_id: String,
    selected_message_id: Option<String>,
    sync_status: SyncStatus,
}

trait MailStorage {
    fn bootstrap(&self, provider_options: Vec<ProviderOption>) -> BootstrapData;
    fn add_account(
        &mut self,
        seed: AccountSeed,
        provider_options: Vec<ProviderOption>,
    ) -> Result<BootstrapData, String>;
    fn select_folder(&mut self, folder_id: &str) -> Result<MailboxSnapshot, String>;
    fn select_message(&mut self, message_id: &str) -> Result<MessageUpdate, String>;
    fn search(&mut self, query: &str) -> MailboxSnapshot;
    fn mark_message_read(&mut self, message_id: &str, read: bool) -> Result<MessageUpdate, String>;
    fn archive_message(&mut self, message_id: &str) -> Result<MailboxSnapshot, String>;
    fn refresh(&mut self, provider_options: Vec<ProviderOption>) -> BootstrapData;
    fn sync_state(&self, account_id: &str) -> Option<ProviderSyncState>;
    fn sync_states(&self) -> Vec<ProviderSyncState>;
    fn save_sync_state(&mut self, state: ProviderSyncState) -> Result<(), String>;
    fn apply_mailbox_delta(&mut self, delta: MailboxDelta) -> Result<(), String>;
    fn enqueue_outbox(&mut self, item: NewOutboxItem) -> Result<SyncOutboxItem, String>;
    fn pending_outbox(&self) -> Vec<SyncOutboxItem>;
    fn outbox_entries(&self) -> Vec<SyncOutboxItem>;
    fn outbox_summary(&self) -> SyncOutboxSummary;
    fn mark_outbox_synced(&mut self, id: &str) -> Result<(), String>;
    fn mark_outbox_failed(&mut self, id: &str, error: String) -> Result<(), String>;
}

impl MailStore {
    fn empty() -> Self {
        Self {
            accounts: Vec::new(),
            folders: Vec::new(),
            messages: Vec::new(),
            credentials: Vec::new(),
            sync_states: Vec::new(),
            outbox: Vec::new(),
            next_outbox_id: 1,
            selected_folder_id: String::new(),
            selected_message_id: None,
            sync_status: SyncStatus {
                state: "Ready".into(),
                last_checked: "-".into(),
                detail: "No account connected".into(),
            },
        }
    }

    fn bootstrap(&self, provider_options: Vec<ProviderOption>) -> BootstrapData {
        let messages = self.messages_for_folder(&self.selected_folder_id);
        BootstrapData {
            provider_options,
            accounts: self.accounts.clone(),
            folders: self.folders.clone(),
            messages,
            selected_folder_id: self.selected_folder_id.clone(),
            selected_message_id: self.selected_message_id.clone(),
            selected_message: self.selected_message(),
            sync_status: self.sync_status.clone(),
        }
    }

    fn add_account(
        &mut self,
        seed: AccountSeed,
        provider_options: Vec<ProviderOption>,
    ) -> Result<BootstrapData, String> {
        let AccountSeed {
            account,
            folders,
            messages,
            credential,
            sync_state,
        } = seed;

        if self
            .accounts
            .iter()
            .any(|existing| existing.id == account.id)
        {
            return Err(format!(
                "Account already connected: {}",
                account.email_address
            ));
        }

        let account_id = account.id.clone();
        let account_email = account.email_address.clone();
        let provider = sync_state.provider;
        let inbox_id = folders
            .iter()
            .find(|folder| folder.account_id == account_id && folder.role == "inbox")
            .map(|folder| folder.id.clone());

        self.accounts.push(account);
        self.folders.extend(folders);
        self.messages.extend(messages);
        self.credentials.push(credential);
        MailStorage::save_sync_state(self, sync_state)?;
        MailStorage::enqueue_outbox(
            self,
            NewOutboxItem {
                account_id: account_id.clone(),
                provider,
                operation: "account.connected".into(),
                payload_json: format!(
                    "{{\"accountId\":\"{account_id}\",\"emailAddress\":\"{account_email}\"}}"
                ),
            },
        )?;
        self.recalculate_unread();

        if inbox_id.is_some() {
            self.selected_folder_id = UNIFIED_INBOX_ID.into();
            self.selected_message_id = self
                .messages_for_folder(&self.selected_folder_id)
                .first()
                .map(|message| message.id.clone());
        }

        self.set_sync_status_from_outbox();

        Ok(self.bootstrap(provider_options))
    }

    fn snapshot(&self, folder_id: &str) -> MailboxSnapshot {
        MailboxSnapshot {
            folder_id: folder_id.into(),
            folders: self.folders.clone(),
            messages: self.messages_for_folder(folder_id),
            selected_message_id: self.selected_message_id.clone(),
            selected_message: self.selected_message(),
            sync_status: self.sync_status.clone(),
        }
    }

    fn selected_message(&self) -> Option<MessageDetail> {
        self.selected_message_id
            .as_ref()
            .and_then(|id| self.messages.iter().find(|message| message.id == *id))
            .cloned()
    }

    fn messages_for_folder(&self, folder_id: &str) -> Vec<MessageSummary> {
        if folder_id == UNIFIED_INBOX_ID {
            let inbox_folder_ids: Vec<&str> = self
                .folders
                .iter()
                .filter(|folder| folder.role == "inbox")
                .map(|folder| folder.id.as_str())
                .collect();

            return self
                .messages
                .iter()
                .filter(|message| inbox_folder_ids.contains(&message.folder_id.as_str()))
                .map(MessageDetail::summary)
                .collect();
        }

        let role = self
            .folders
            .iter()
            .find(|folder| folder.id == folder_id)
            .map(|folder| folder.role.as_str())
            .unwrap_or("inbox");

        self.messages
            .iter()
            .filter(|message| match role {
                "starred" => message.starred,
                _ => message.folder_id == folder_id,
            })
            .map(MessageDetail::summary)
            .collect()
    }

    fn select_folder(&mut self, folder_id: &str) -> Result<MailboxSnapshot, String> {
        if folder_id != UNIFIED_INBOX_ID
            && !self.folders.iter().any(|folder| folder.id == folder_id)
        {
            return Err(format!("Unknown folder: {folder_id}"));
        }

        self.selected_folder_id = folder_id.into();
        self.selected_message_id = self
            .messages_for_folder(folder_id)
            .first()
            .map(|message| message.id.clone());

        Ok(self.snapshot(folder_id))
    }

    fn select_message(&mut self, message_id: &str) -> Result<MessageUpdate, String> {
        let was_unread = self.message_by_id(message_id)?.unread;
        self.set_read_state(message_id, false)?;
        if was_unread {
            self.queue_message_mutation(
                message_id,
                "message.mark_read",
                format!("{{\"messageId\":\"{message_id}\",\"read\":true}}"),
            )?;
        }
        self.selected_message_id = Some(message_id.into());
        let message = self.message_by_id(message_id)?;
        Ok(MessageUpdate {
            folders: self.folders.clone(),
            message,
            sync_status: self.sync_status.clone(),
        })
    }

    fn set_read_state(&mut self, message_id: &str, unread: bool) -> Result<(), String> {
        let message = self
            .messages
            .iter_mut()
            .find(|message| message.id == message_id)
            .ok_or_else(|| format!("Unknown message: {message_id}"))?;
        message.unread = unread;
        self.recalculate_unread();
        Ok(())
    }

    fn mark_message_read(&mut self, message_id: &str, read: bool) -> Result<MessageUpdate, String> {
        let was_read = !self.message_by_id(message_id)?.unread;
        self.set_read_state(message_id, !read)?;
        if was_read != read {
            self.queue_message_mutation(
                message_id,
                "message.mark_read",
                format!("{{\"messageId\":\"{message_id}\",\"read\":{read}}}"),
            )?;
        }
        let message = self.message_by_id(message_id)?;
        Ok(MessageUpdate {
            folders: self.folders.clone(),
            message,
            sync_status: self.sync_status.clone(),
        })
    }

    fn archive_message(&mut self, message_id: &str) -> Result<MailboxSnapshot, String> {
        let previous_folder = self.selected_folder_id.clone();
        let account_id = self
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .map(|message| message.account_id.clone())
            .ok_or_else(|| format!("Unknown message: {message_id}"))?;

        let archive_folder_id = self
            .folders
            .iter()
            .find(|folder| folder.account_id == account_id && folder.role == "archive")
            .map(|folder| folder.id.clone())
            .unwrap_or_else(|| "archive".into());

        let message = self
            .messages
            .iter_mut()
            .find(|message| message.id == message_id)
            .ok_or_else(|| format!("Unknown message: {message_id}"))?;

        message.folder_id = archive_folder_id;
        message.unread = false;
        self.recalculate_unread();
        self.queue_message_mutation(
            message_id,
            "message.archive",
            format!("{{\"messageId\":\"{message_id}\"}}"),
        )?;

        self.selected_message_id = self
            .messages_for_folder(&previous_folder)
            .first()
            .map(|message| message.id.clone());

        Ok(self.snapshot(&previous_folder))
    }

    fn search(&mut self, query: &str) -> MailboxSnapshot {
        let normalized = query.trim().to_lowercase();
        let matches = if normalized.is_empty() {
            self.messages_for_folder(&self.selected_folder_id)
        } else {
            self.messages
                .iter()
                .filter(|message| {
                    let haystack = format!(
                        "{} {} {} {} {}",
                        message.sender_name,
                        message.sender_email,
                        message.subject,
                        message.snippet,
                        message.labels.join(" ")
                    )
                    .to_lowercase();
                    haystack.contains(&normalized)
                })
                .map(MessageDetail::summary)
                .collect()
        };

        self.selected_message_id = matches.first().map(|message| message.id.clone());
        MailboxSnapshot {
            folder_id: if normalized.is_empty() {
                self.selected_folder_id.clone()
            } else {
                "search".into()
            },
            folders: self.folders.clone(),
            messages: matches,
            selected_message_id: self.selected_message_id.clone(),
            selected_message: self.selected_message(),
            sync_status: self.sync_status.clone(),
        }
    }

    fn refresh(&mut self, provider_options: Vec<ProviderOption>) -> BootstrapData {
        self.set_sync_status_from_outbox();
        self.bootstrap(provider_options)
    }

    fn message_by_id(&self, message_id: &str) -> Result<MessageDetail, String> {
        self.messages
            .iter()
            .find(|message| message.id == message_id)
            .cloned()
            .ok_or_else(|| format!("Unknown message: {message_id}"))
    }

    fn sync_state(&self, account_id: &str) -> Option<ProviderSyncState> {
        self.sync_states
            .iter()
            .find(|state| state.account_id == account_id)
            .cloned()
    }

    fn sync_states(&self) -> Vec<ProviderSyncState> {
        self.sync_states.clone()
    }

    fn save_sync_state(&mut self, state: ProviderSyncState) -> Result<(), String> {
        if !self
            .accounts
            .iter()
            .any(|account| account.id == state.account_id)
        {
            return Err(format!("Unknown account: {}", state.account_id));
        }

        match self
            .sync_states
            .iter()
            .position(|existing| existing.account_id == state.account_id)
        {
            Some(index) => self.sync_states[index] = state,
            None => self.sync_states.push(state),
        }

        Ok(())
    }

    fn apply_mailbox_delta(&mut self, delta: MailboxDelta) -> Result<(), String> {
        let account_id = delta.sync_state.account_id.clone();
        if !self.accounts.iter().any(|account| account.id == account_id) {
            return Err(format!("Unknown account: {account_id}"));
        }

        for folder in delta.folders {
            match self
                .folders
                .iter()
                .position(|existing| existing.id == folder.id)
            {
                Some(index) => self.folders[index] = folder,
                None => self.folders.push(folder),
            }
        }

        let message_count = delta.messages.len();
        for message in delta.messages {
            match self
                .messages
                .iter()
                .position(|existing| existing.id == message.id)
            {
                Some(index) => self.messages[index] = message,
                None => self.messages.push(message),
            }
        }

        MailStorage::save_sync_state(self, delta.sync_state)?;
        self.recalculate_unread();
        if message_count > 0 {
            self.sync_status = SyncStatus {
                state: "Synced".into(),
                last_checked: "Just now".into(),
                detail: format!(
                    "{} messages cached locally; {} new from provider",
                    self.messages.len(),
                    message_count
                ),
            };
        }
        Ok(())
    }

    fn enqueue_outbox(&mut self, item: NewOutboxItem) -> Result<SyncOutboxItem, String> {
        if !self
            .accounts
            .iter()
            .any(|account| account.id == item.account_id)
        {
            return Err(format!("Unknown account: {}", item.account_id));
        }

        let outbox_item = SyncOutboxItem {
            id: format!("outbox-{}", self.next_outbox_id),
            account_id: item.account_id,
            provider: item.provider,
            operation: item.operation,
            payload_json: item.payload_json,
            status: "pending".into(),
            attempt_count: 0,
            created_at: "Now".into(),
            last_attempted_at: None,
            last_synced_at: None,
            last_error: None,
        };
        self.next_outbox_id += 1;
        self.outbox.push(outbox_item.clone());
        self.set_sync_status_from_outbox();
        Ok(outbox_item)
    }

    fn pending_outbox(&self) -> Vec<SyncOutboxItem> {
        self.outbox
            .iter()
            .filter(|item| item.status == "pending")
            .cloned()
            .collect()
    }

    fn outbox_entries(&self) -> Vec<SyncOutboxItem> {
        self.outbox.clone()
    }

    fn outbox_summary(&self) -> SyncOutboxSummary {
        let mut summary = SyncOutboxSummary {
            pending: 0,
            synced: 0,
            failed: 0,
        };

        for item in &self.outbox {
            match item.status.as_str() {
                "pending" => summary.pending += 1,
                "synced" => summary.synced += 1,
                "failed" => summary.failed += 1,
                _ => {}
            }
        }

        summary
    }

    fn mark_outbox_synced(&mut self, id: &str) -> Result<(), String> {
        let item = self
            .outbox
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| format!("Unknown outbox item: {id}"))?;

        item.status = "synced".into();
        item.attempt_count += 1;
        item.last_attempted_at = Some("Just now".into());
        item.last_synced_at = Some("Just now".into());
        item.last_error = None;
        self.set_sync_status_from_outbox();
        Ok(())
    }

    fn mark_outbox_failed(&mut self, id: &str, error: String) -> Result<(), String> {
        let item = self
            .outbox
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| format!("Unknown outbox item: {id}"))?;

        item.status = "failed".into();
        item.attempt_count += 1;
        item.last_attempted_at = Some("Just now".into());
        item.last_error = Some(error);
        self.set_sync_status_from_outbox();
        Ok(())
    }

    fn provider_for_account(&self, account_id: &str) -> Result<ProviderKind, String> {
        self.sync_state(account_id)
            .map(|state| state.provider)
            .ok_or_else(|| format!("No provider sync state for account: {account_id}"))
    }

    fn queue_message_mutation(
        &mut self,
        message_id: &str,
        operation: &str,
        payload_json: String,
    ) -> Result<(), String> {
        let account_id = self.message_by_id(message_id)?.account_id;
        let provider = self.provider_for_account(&account_id)?;
        MailStorage::enqueue_outbox(
            self,
            NewOutboxItem {
                account_id,
                provider,
                operation: operation.into(),
                payload_json,
            },
        )?;
        Ok(())
    }

    fn set_sync_status_from_outbox(&mut self) {
        let summary = self.outbox_summary();
        self.sync_status = if summary.pending > 0 {
            SyncStatus {
                state: "Local".into(),
                last_checked: "Not synced".into(),
                detail: format!(
                    "{} messages cached locally; {} change{} pending sync",
                    self.messages.len(),
                    summary.pending,
                    if summary.pending == 1 { "" } else { "s" }
                ),
            }
        } else if summary.failed > 0 {
            SyncStatus {
                state: "Needs retry".into(),
                last_checked: "Just now".into(),
                detail: format!(
                    "{} messages cached locally; {} sync failure{}",
                    self.messages.len(),
                    summary.failed,
                    if summary.failed == 1 { "" } else { "s" }
                ),
            }
        } else {
            SyncStatus {
                state: "Synced".into(),
                last_checked: "Just now".into(),
                detail: format!("{} messages cached locally", self.messages.len()),
            }
        };
    }

    fn recalculate_unread(&mut self) {
        for folder in &mut self.folders {
            let role = folder.role.as_str();
            folder.unread_count = self
                .messages
                .iter()
                .filter(|message| {
                    message.unread
                        && match role {
                            "starred" => message.starred,
                            _ => message.folder_id == folder.id,
                        }
                })
                .count();
        }
    }
}

impl MailStorage for MailStore {
    fn bootstrap(&self, provider_options: Vec<ProviderOption>) -> BootstrapData {
        MailStore::bootstrap(self, provider_options)
    }

    fn add_account(
        &mut self,
        seed: AccountSeed,
        provider_options: Vec<ProviderOption>,
    ) -> Result<BootstrapData, String> {
        MailStore::add_account(self, seed, provider_options)
    }

    fn select_folder(&mut self, folder_id: &str) -> Result<MailboxSnapshot, String> {
        MailStore::select_folder(self, folder_id)
    }

    fn select_message(&mut self, message_id: &str) -> Result<MessageUpdate, String> {
        MailStore::select_message(self, message_id)
    }

    fn search(&mut self, query: &str) -> MailboxSnapshot {
        MailStore::search(self, query)
    }

    fn mark_message_read(&mut self, message_id: &str, read: bool) -> Result<MessageUpdate, String> {
        MailStore::mark_message_read(self, message_id, read)
    }

    fn archive_message(&mut self, message_id: &str) -> Result<MailboxSnapshot, String> {
        MailStore::archive_message(self, message_id)
    }

    fn refresh(&mut self, provider_options: Vec<ProviderOption>) -> BootstrapData {
        MailStore::refresh(self, provider_options)
    }

    fn sync_state(&self, account_id: &str) -> Option<ProviderSyncState> {
        MailStore::sync_state(self, account_id)
    }

    fn sync_states(&self) -> Vec<ProviderSyncState> {
        MailStore::sync_states(self)
    }

    fn save_sync_state(&mut self, state: ProviderSyncState) -> Result<(), String> {
        MailStore::save_sync_state(self, state)
    }

    fn apply_mailbox_delta(&mut self, delta: MailboxDelta) -> Result<(), String> {
        MailStore::apply_mailbox_delta(self, delta)
    }

    fn enqueue_outbox(&mut self, item: NewOutboxItem) -> Result<SyncOutboxItem, String> {
        MailStore::enqueue_outbox(self, item)
    }

    fn pending_outbox(&self) -> Vec<SyncOutboxItem> {
        MailStore::pending_outbox(self)
    }

    fn outbox_entries(&self) -> Vec<SyncOutboxItem> {
        MailStore::outbox_entries(self)
    }

    fn outbox_summary(&self) -> SyncOutboxSummary {
        MailStore::outbox_summary(self)
    }

    fn mark_outbox_synced(&mut self, id: &str) -> Result<(), String> {
        MailStore::mark_outbox_synced(self, id)
    }

    fn mark_outbox_failed(&mut self, id: &str, error: String) -> Result<(), String> {
        MailStore::mark_outbox_failed(self, id, error)
    }
}

trait MailProvider: Send + Sync {
    fn option(&self) -> ProviderOption;
    fn begin_login(&self) -> Result<AccountLogin, String>;
    fn complete_login(&self, login_state: &str, email_address: &str)
        -> Result<AccountSeed, String>;
    fn sync_mailbox(&self, state: &ProviderSyncState) -> Result<MailboxDelta, String>;
}

struct ProviderRegistry {
    providers: HashMap<ProviderKind, Box<dyn MailProvider>>,
}

impl ProviderRegistry {
    fn mock() -> Self {
        let mut providers: HashMap<ProviderKind, Box<dyn MailProvider>> = HashMap::new();
        providers.insert(
            ProviderKind::MockGmail,
            Box::new(MockMailProvider {
                kind: ProviderKind::MockGmail,
                display_name: "Mock Gmail",
                description: "OAuth-style setup with Gmail-shaped mailbox data",
                account_label: "Gmail",
                domain: "gmail.test",
                accent: "#c24135",
            }),
        );
        providers.insert(
            ProviderKind::MockOutlook,
            Box::new(MockMailProvider {
                kind: ProviderKind::MockOutlook,
                display_name: "Mock Outlook",
                description: "OAuth-style setup with Outlook-shaped mailbox data",
                account_label: "Outlook",
                domain: "outlook.test",
                accent: "#2f5f9f",
            }),
        );
        Self { providers }
    }

    fn options(&self) -> Vec<ProviderOption> {
        let mut options: Vec<ProviderOption> = self
            .providers
            .values()
            .map(|provider| provider.option())
            .collect();
        options.sort_by(|left, right| left.display_name.cmp(&right.display_name));
        options
    }

    fn get(&self, provider: ProviderKind) -> Result<&dyn MailProvider, String> {
        self.providers
            .get(&provider)
            .map(|provider| provider.as_ref())
            .ok_or_else(|| format!("Unsupported provider: {provider:?}"))
    }
}

struct MockMailProvider {
    kind: ProviderKind,
    display_name: &'static str,
    description: &'static str,
    account_label: &'static str,
    domain: &'static str,
    accent: &'static str,
}

impl MailProvider for MockMailProvider {
    fn option(&self) -> ProviderOption {
        ProviderOption {
            provider: self.kind,
            display_name: self.display_name.into(),
            description: self.description.into(),
        }
    }

    fn begin_login(&self) -> Result<AccountLogin, String> {
        Ok(AccountLogin {
            provider: self.kind,
            login_url: format!("lotus://mock-auth/{}", provider_slug(self.kind)),
            login_state: format!("{}-oauth-state", provider_slug(self.kind)),
            expires_at: "10 minutes from now".into(),
            scopes: vec![
                "openid".into(),
                "email".into(),
                "mail.read".into(),
                "offline_access".into(),
            ],
        })
    }

    fn complete_login(
        &self,
        login_state: &str,
        email_address: &str,
    ) -> Result<AccountSeed, String> {
        let email = normalize_email(email_address, self.domain)?;
        let account_id = format!("{}-{}", provider_slug(self.kind), account_slug(&email));
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
            accent: self.accent.into(),
        };

        let folders = vec![
            folder(&inbox_id, &account_id, "Inbox", "inbox"),
            folder(&starred_id, &account_id, "Starred", "starred"),
            folder(&drafts_id, &account_id, "Drafts", "drafts"),
            folder(&sent_id, &account_id, "Sent", "sent"),
            folder(&archive_id, &account_id, "Archive", "archive"),
        ];

        let messages = vec![
            message(
                &format!("{account_id}-welcome"),
                &account_id,
                &inbox_id,
                "Lotus Account Setup",
                "setup@lotus.local",
                &format!("{} connected to Lotus", self.display_name),
                "Your mock OAuth flow completed and the first mailbox sync is ready.",
                "Just now",
                true,
                true,
                vec![self.account_label, "Setup"],
                vec![email.as_str()],
                vec![],
                vec![
                    format!(
                        "The mock authorization callback returned an access token and refresh token for {email}."
                    ),
                    "Lotus stored the credential in the local in-memory credential store and imported this initial mailbox snapshot.".into(),
                ],
            ),
            message(
                &format!("{account_id}-digest"),
                &account_id,
                &inbox_id,
                "Product Updates",
                &format!("updates@{}", self.domain),
                "Your morning digest",
                "Three threads are waiting, including one invoice and one calendar update.",
                "Today 8:10 AM",
                true,
                false,
                vec![self.account_label],
                vec![email.as_str()],
                vec![],
                vec![
                    "Three threads are waiting in the mock mailbox. This message exists to exercise unread counts, snippets, and message detail rendering.".into(),
                    "Provider-specific sync code can replace this seed data later without changing the Elm dashboard contract.".into(),
                ],
            ),
            message(
                &format!("{account_id}-receipt"),
                &account_id,
                &archive_id,
                "Billing",
                &format!("billing@{}", self.domain),
                "Receipt for your workspace",
                "The receipt was imported directly into Archive during initial sync.",
                "Yesterday",
                false,
                false,
                vec!["Receipts"],
                vec![email.as_str()],
                vec![],
                vec![
                    "This archived message verifies that provider imports can seed folders beyond the inbox.".into(),
                    "A real provider would produce the same normalized message shape after fetching remote data.".into(),
                ],
            ),
        ];

        let credential = StoredCredential {
            account_id: account_id.clone(),
            provider: self.kind,
            access_token: format!("mock-access-{}-{login_state}", provider_slug(self.kind)),
            refresh_token: format!("mock-refresh-{}-{login_state}", provider_slug(self.kind)),
            expires_at: "1 hour from now".into(),
        };

        let sync_state = ProviderSyncState {
            account_id: account_id.clone(),
            provider: self.kind,
            remote_account_id: email,
            sync_cursor: Some(format!("{}-cursor-1", provider_slug(self.kind))),
            history_id: Some(format!("{}-history-1", provider_slug(self.kind))),
            delta_token: Some(format!("{}-delta-1", provider_slug(self.kind))),
            page_token: None,
            high_watermark: Some("Just now".into()),
            status: "idle".into(),
            last_attempted_at: Some("Just now".into()),
            last_successful_at: Some("Just now".into()),
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

    fn sync_mailbox(&self, state: &ProviderSyncState) -> Result<MailboxDelta, String> {
        let account_id = state.account_id.clone();
        let inbox_id = format!("{account_id}-inbox");
        let mut next_state = state.clone();
        next_state.status = "idle".into();
        next_state.last_attempted_at = Some("Just now".into());
        next_state.last_successful_at = Some("Just now".into());
        next_state.last_error = None;

        let first_cursor = format!("{}-cursor-1", provider_slug(self.kind));
        let messages = if state.sync_cursor.as_deref() == Some(first_cursor.as_str()) {
            next_state.sync_cursor = Some(format!("{}-cursor-2", provider_slug(self.kind)));
            next_state.history_id = Some(format!("{}-history-2", provider_slug(self.kind)));
            next_state.delta_token = Some(format!("{}-delta-2", provider_slug(self.kind)));
            next_state.high_watermark = Some("After first provider sync".into());

            vec![message(
                    &format!("{account_id}-provider-sync-2"),
                    &account_id,
                    &inbox_id,
                    "Inbox Sync",
                    &format!("sync@{}", self.domain),
                    "New message cached for offline reading",
                    "The sync engine imported this provider message into the local mailbox cache.",
                    "Just now",
                    true,
                    false,
                    vec![self.account_label, "Synced"],
                    vec![state.remote_account_id.as_str()],
                    vec![],
                    vec![
                        "This message arrived through the provider sync path, not through account setup.".into(),
                        "Because it is stored in the local mailbox cache, Lotus can render it while offline.".into(),
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
    }
}

trait SyncEngine: Send + Sync {
    fn sync_once(
        &self,
        storage: &mut dyn MailStorage,
        providers: &ProviderRegistry,
    ) -> Result<SyncReport, String>;
}

struct LocalFirstSyncEngine;

impl SyncEngine for LocalFirstSyncEngine {
    fn sync_once(
        &self,
        storage: &mut dyn MailStorage,
        providers: &ProviderRegistry,
    ) -> Result<SyncReport, String> {
        let pending = storage.pending_outbox();
        let attempted = pending.len();
        let mut synced = 0;
        let mut failed = 0;
        let mut inbound_messages = 0;

        for sync_state in storage.sync_states() {
            let provider = match providers.get(sync_state.provider) {
                Ok(provider) => provider,
                Err(error) => {
                    storage.save_sync_state(failed_sync_state(sync_state, error))?;
                    failed += 1;
                    continue;
                }
            };

            match provider.sync_mailbox(&sync_state) {
                Ok(delta) => {
                    inbound_messages += delta.messages.len();
                    storage.apply_mailbox_delta(delta)?;
                }
                Err(error) => {
                    storage.save_sync_state(failed_sync_state(sync_state, error))?;
                    failed += 1;
                }
            }
        }

        for item in pending {
            match storage.mark_outbox_synced(&item.id) {
                Ok(()) => synced += 1,
                Err(error) => {
                    failed += 1;
                    let _ = storage.mark_outbox_failed(&item.id, error);
                }
            }
        }

        let remaining_pending = storage.outbox_summary().pending;
        Ok(SyncReport {
            attempted,
            synced,
            failed,
            inbound_messages,
            remaining_pending,
        })
    }
}

struct AppState {
    storage: Mutex<MailStore>,
    providers: ProviderRegistry,
    sync_engine: LocalFirstSyncEngine,
}

#[tauri::command]
fn app_bootstrap(state: tauri::State<'_, AppState>) -> Result<BootstrapData, String> {
    let storage = state.storage.lock().map_err(|error| error.to_string())?;
    Ok(MailStorage::bootstrap(&*storage, state.providers.options()))
}

#[tauri::command]
fn begin_account_login(
    state: tauri::State<'_, AppState>,
    provider: ProviderKind,
) -> Result<AccountLogin, String> {
    state.providers.get(provider)?.begin_login()
}

#[tauri::command]
fn complete_account_login(
    state: tauri::State<'_, AppState>,
    provider: ProviderKind,
    login_state: String,
    email_address: String,
) -> Result<AccountSetupResult, String> {
    let seed = state
        .providers
        .get(provider)?
        .complete_login(&login_state, &email_address)?;
    let credential = seed.credential.preview();

    let mut storage = state.storage.lock().map_err(|error| error.to_string())?;
    let bootstrap = MailStorage::add_account(&mut *storage, seed, state.providers.options())?;
    Ok(AccountSetupResult {
        bootstrap,
        credential,
    })
}

#[tauri::command]
fn provider_sync_checkpoint(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<ProviderSyncCheckpoint, String> {
    let storage = state.storage.lock().map_err(|error| error.to_string())?;
    MailStorage::sync_state(&*storage, &account_id)
        .map(ProviderSyncCheckpoint::from)
        .ok_or_else(|| format!("No provider sync checkpoint for account: {account_id}"))
}

#[tauri::command]
fn sync_pending_changes(state: tauri::State<'_, AppState>) -> Result<SyncReport, String> {
    let mut storage = state.storage.lock().map_err(|error| error.to_string())?;
    state.sync_engine.sync_once(&mut *storage, &state.providers)
}

#[tauri::command]
fn sync_outbox_status(state: tauri::State<'_, AppState>) -> Result<SyncOutboxSummary, String> {
    let storage = state.storage.lock().map_err(|error| error.to_string())?;
    Ok(MailStorage::outbox_summary(&*storage))
}

#[tauri::command]
fn sync_outbox_entries(state: tauri::State<'_, AppState>) -> Result<Vec<SyncOutboxEntry>, String> {
    let storage = state.storage.lock().map_err(|error| error.to_string())?;
    Ok(MailStorage::outbox_entries(&*storage)
        .into_iter()
        .map(SyncOutboxEntry::from)
        .collect())
}

#[tauri::command]
fn select_folder(
    state: tauri::State<'_, AppState>,
    folder_id: String,
) -> Result<MailboxSnapshot, String> {
    let mut storage = state.storage.lock().map_err(|error| error.to_string())?;
    MailStorage::select_folder(&mut *storage, &folder_id)
}

#[tauri::command]
fn select_message(
    state: tauri::State<'_, AppState>,
    message_id: String,
) -> Result<MessageUpdate, String> {
    let mut storage = state.storage.lock().map_err(|error| error.to_string())?;
    MailStorage::select_message(&mut *storage, &message_id)
}

#[tauri::command]
fn search_messages(
    state: tauri::State<'_, AppState>,
    query: String,
) -> Result<MailboxSnapshot, String> {
    let mut storage = state.storage.lock().map_err(|error| error.to_string())?;
    Ok(MailStorage::search(&mut *storage, &query))
}

#[tauri::command]
fn mark_message_read(
    state: tauri::State<'_, AppState>,
    message_id: String,
    read: bool,
) -> Result<MessageUpdate, String> {
    let mut storage = state.storage.lock().map_err(|error| error.to_string())?;
    MailStorage::mark_message_read(&mut *storage, &message_id, read)
}

#[tauri::command]
fn archive_message(
    state: tauri::State<'_, AppState>,
    message_id: String,
) -> Result<MailboxSnapshot, String> {
    let mut storage = state.storage.lock().map_err(|error| error.to_string())?;
    MailStorage::archive_message(&mut *storage, &message_id)
}

#[tauri::command]
fn refresh_mail(state: tauri::State<'_, AppState>) -> Result<BootstrapData, String> {
    let mut storage = state.storage.lock().map_err(|error| error.to_string())?;
    state
        .sync_engine
        .sync_once(&mut *storage, &state.providers)?;
    Ok(MailStorage::refresh(
        &mut *storage,
        state.providers.options(),
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            storage: Mutex::new(MailStore::empty()),
            providers: ProviderRegistry::mock(),
            sync_engine: LocalFirstSyncEngine,
        })
        .invoke_handler(tauri::generate_handler![
            app_bootstrap,
            begin_account_login,
            complete_account_login,
            provider_sync_checkpoint,
            sync_pending_changes,
            sync_outbox_status,
            sync_outbox_entries,
            select_folder,
            select_message,
            search_messages,
            mark_message_read,
            archive_message,
            refresh_mail
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lotus");
}

fn folder(id: &str, account_id: &str, name: &str, role: &str) -> Folder {
    Folder {
        id: id.into(),
        account_id: account_id.into(),
        name: name.into(),
        role: role.into(),
        unread_count: 0,
    }
}

#[allow(clippy::too_many_arguments)]
fn message(
    id: &str,
    account_id: &str,
    folder_id: &str,
    sender_name: &str,
    sender_email: &str,
    subject: &str,
    snippet: &str,
    received_at: &str,
    unread: bool,
    starred: bool,
    labels: Vec<&str>,
    to: Vec<&str>,
    cc: Vec<&str>,
    body_paragraphs: Vec<String>,
) -> MessageDetail {
    MessageDetail {
        id: id.into(),
        account_id: account_id.into(),
        folder_id: folder_id.into(),
        sender_name: sender_name.into(),
        sender_email: sender_email.into(),
        subject: subject.into(),
        snippet: snippet.into(),
        received_at: received_at.into(),
        unread,
        starred,
        labels: labels.into_iter().map(String::from).collect(),
        to: to.into_iter().map(String::from).collect(),
        cc: cc.into_iter().map(String::from).collect(),
        body_paragraphs,
    }
}

fn provider_slug(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::MockGmail => "mock-gmail",
        ProviderKind::MockOutlook => "mock-outlook",
    }
}

fn failed_sync_state(mut state: ProviderSyncState, error: String) -> ProviderSyncState {
    state.status = "failed".into();
    state.last_attempted_at = Some("Just now".into());
    state.last_error = Some(error);
    state
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

fn token_tail(token: &str) -> String {
    let mut tail: Vec<char> = token.chars().rev().take(6).collect();
    tail.reverse();
    tail.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_provider_login_imports_mailbox_and_stores_credential() {
        let providers = ProviderRegistry::mock();
        let provider = providers.get(ProviderKind::MockGmail).unwrap();
        let login = provider.begin_login().unwrap();
        let seed = provider
            .complete_login(&login.login_state, "reader@gmail.test")
            .unwrap();
        let credential = seed.credential.preview();

        let mut store = MailStore::empty();
        let bootstrap = store.add_account(seed, providers.options()).unwrap();

        assert_eq!(bootstrap.accounts.len(), 1);
        assert_eq!(bootstrap.folders.len(), 5);
        assert_eq!(bootstrap.messages.len(), 2);
        assert_eq!(store.credentials.len(), 1);
        assert_eq!(MailStorage::outbox_summary(&store).pending, 1);
        assert_eq!(credential.refresh_token_tail, "-state");
        assert_eq!(bootstrap.selected_folder_id, UNIFIED_INBOX_ID);
        assert!(bootstrap.selected_message_id.is_some());

        let mut sync_state =
            MailStorage::sync_state(&store, "mock-gmail-reader-gmail-test").unwrap();
        assert_eq!(sync_state.provider, ProviderKind::MockGmail);
        assert_eq!(sync_state.remote_account_id, "reader@gmail.test");
        assert_eq!(
            sync_state.sync_cursor.as_deref(),
            Some("mock-gmail-cursor-1")
        );
        assert_eq!(
            sync_state.history_id.as_deref(),
            Some("mock-gmail-history-1")
        );
        assert_eq!(
            sync_state.delta_token.as_deref(),
            Some("mock-gmail-delta-1")
        );
        assert_eq!(sync_state.page_token, None);
        assert_eq!(sync_state.high_watermark.as_deref(), Some("Just now"));
        assert_eq!(sync_state.status, "idle");
        assert_eq!(sync_state.last_attempted_at.as_deref(), Some("Just now"));
        assert_eq!(sync_state.last_successful_at.as_deref(), Some("Just now"));
        assert_eq!(sync_state.last_error, None);

        sync_state.sync_cursor = Some("mock-gmail-cursor-2".into());
        sync_state.high_watermark = Some("After first poll".into());
        MailStorage::save_sync_state(&mut store, sync_state).unwrap();
        let advanced = MailStorage::sync_state(&store, "mock-gmail-reader-gmail-test").unwrap();
        assert_eq!(advanced.sync_cursor.as_deref(), Some("mock-gmail-cursor-2"));
        assert_eq!(advanced.high_watermark.as_deref(), Some("After first poll"));
    }

    #[test]
    fn local_mutations_enqueue_outbox_and_sync_engine_drains_pending_work() {
        let providers = ProviderRegistry::mock();
        let provider = providers.get(ProviderKind::MockGmail).unwrap();
        let login = provider.begin_login().unwrap();
        let seed = provider
            .complete_login(&login.login_state, "reader@gmail.test")
            .unwrap();

        let mut store = MailStore::empty();
        store.add_account(seed, providers.options()).unwrap();
        let engine = LocalFirstSyncEngine;
        let initial_report = engine.sync_once(&mut store, &providers).unwrap();
        assert_eq!(initial_report.attempted, 1);
        assert_eq!(initial_report.inbound_messages, 1);
        assert_eq!(initial_report.remaining_pending, 0);

        let inbox = store.messages_for_folder("mock-gmail-reader-gmail-test-inbox");
        assert_eq!(inbox.len(), 3);
        assert!(inbox
            .iter()
            .any(|message| message.id == "mock-gmail-reader-gmail-test-provider-sync-2"));
        let sync_state = MailStorage::sync_state(&store, "mock-gmail-reader-gmail-test").unwrap();
        assert_eq!(
            sync_state.sync_cursor.as_deref(),
            Some("mock-gmail-cursor-2")
        );

        store
            .select_message("mock-gmail-reader-gmail-test-welcome")
            .unwrap();
        store
            .archive_message("mock-gmail-reader-gmail-test-welcome")
            .unwrap();

        let queued = MailStorage::outbox_summary(&store);
        assert_eq!(queued.pending, 2);
        assert_eq!(queued.synced, 1);

        let report = engine.sync_once(&mut store, &providers).unwrap();
        assert_eq!(report.attempted, 2);
        assert_eq!(report.synced, 2);
        assert_eq!(report.failed, 0);
        assert_eq!(report.inbound_messages, 0);
        assert_eq!(report.remaining_pending, 0);

        let final_summary = MailStorage::outbox_summary(&store);
        assert_eq!(final_summary.pending, 0);
        assert_eq!(final_summary.synced, 3);
    }

    #[test]
    fn duplicate_mock_account_is_rejected() {
        let providers = ProviderRegistry::mock();
        let provider = providers.get(ProviderKind::MockOutlook).unwrap();
        let login = provider.begin_login().unwrap();

        let mut store = MailStore::empty();
        let first_seed = provider
            .complete_login(&login.login_state, "reader@outlook.test")
            .unwrap();
        store.add_account(first_seed, providers.options()).unwrap();

        let second_seed = provider
            .complete_login(&login.login_state, "reader@outlook.test")
            .unwrap();
        let error = match store.add_account(second_seed, providers.options()) {
            Ok(_) => panic!("duplicate account should be rejected"),
            Err(error) => error,
        };

        assert!(error.contains("Account already connected"));
    }
}
