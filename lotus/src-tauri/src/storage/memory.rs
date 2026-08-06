//! In-memory mailbox storage. Kept after Phase 1 because it makes provider and
//! sync-engine tests fast and free of a filesystem.

use crate::model::*;
use crate::storage::MailStorage;

pub struct MailStore {
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

impl MailStore {
    pub fn empty() -> Self {
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
                last_checked: None,
                detail: "No account connected".into(),
            },
        }
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

    /// Membership matching over `folder_ids`, deduped by message id. A Gmail
    /// message in `INBOX` and a user label matches two inbox-role folders in the
    /// unified pass, and would otherwise appear twice in the list pane.
    pub fn messages_for_folder(&self, folder_id: &str) -> Vec<MessageSummary> {
        let mut matched: Vec<&MessageDetail> = if folder_id == UNIFIED_INBOX_ID {
            let inbox_folder_ids: Vec<&str> = self
                .folders
                .iter()
                .filter(|folder| folder.role == "inbox")
                .map(|folder| folder.id.as_str())
                .collect();

            self.messages
                .iter()
                .filter(|message| {
                    inbox_folder_ids
                        .iter()
                        .any(|inbox_id| message.in_folder(inbox_id))
                })
                .collect()
        } else {
            let folder = self.folders.iter().find(|folder| folder.id == folder_id);
            let role = folder.map(|folder| folder.role.as_str()).unwrap_or("inbox");
            let account_id = folder
                .map(|folder| folder.account_id.as_str())
                .unwrap_or("");

            self.messages
                .iter()
                .filter(|message| match role {
                    // Starred reads the flag, not a membership, so scope it to
                    // the folder's own account.
                    "starred" => message.starred && message.account_id == account_id,
                    _ => message.in_folder(folder_id),
                })
                .collect()
        };

        let mut seen: Vec<&str> = Vec::with_capacity(matched.len());
        matched.retain(|message| {
            if seen.contains(&message.id.as_str()) {
                false
            } else {
                seen.push(message.id.as_str());
                true
            }
        });

        matched.sort_by(|left, right| right.internal_date.cmp(&left.internal_date));
        matched.iter().map(|message| message.summary()).collect()
    }

    fn message_by_id(&self, message_id: &str) -> Result<MessageDetail, String> {
        self.messages
            .iter()
            .find(|message| message.id == message_id)
            .cloned()
            .ok_or_else(|| format!("Unknown message: {message_id}"))
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
        self.enqueue_outbox(NewOutboxItem {
            account_id,
            provider,
            operation: operation.into(),
            payload_json,
        })?;
        Ok(())
    }

    fn set_sync_status_from_outbox(&mut self) {
        let summary = self.outbox_summary();
        let cached = self.messages.len();
        self.sync_status = if summary.pending > 0 {
            SyncStatus {
                state: "Local".into(),
                last_checked: None,
                detail: format!(
                    "{} messages cached locally; {} change{} pending sync",
                    cached,
                    summary.pending,
                    if summary.pending == 1 { "" } else { "s" }
                ),
            }
        } else if summary.failed > 0 {
            SyncStatus {
                state: "Needs retry".into(),
                last_checked: Some(now_iso8601()),
                detail: format!(
                    "{} messages cached locally; {} sync failure{}",
                    cached,
                    summary.failed,
                    if summary.failed == 1 { "" } else { "s" }
                ),
            }
        } else {
            SyncStatus {
                state: "Synced".into(),
                last_checked: Some(now_iso8601()),
                detail: format!("{cached} messages cached locally"),
            }
        };
    }

    /// Unread counts over many-to-many membership.
    fn recalculate_unread(&mut self) {
        for folder in &mut self.folders {
            let role = folder.role.as_str();
            let folder_id = folder.id.as_str();
            let account_id = folder.account_id.as_str();
            folder.unread_count = self
                .messages
                .iter()
                .filter(|message| {
                    message.unread
                        && match role {
                            "starred" => message.starred && message.account_id == account_id,
                            _ => message.in_folder(folder_id),
                        }
                })
                .count();
        }
    }

    fn inbox_folder_id(&self, account_id: &str) -> Option<String> {
        self.folders
            .iter()
            .find(|folder| folder.account_id == account_id && folder.role == "inbox")
            .map(|folder| folder.id.clone())
    }
}

impl MailStorage for MailStore {
    fn bootstrap(&self, provider_options: Vec<ProviderOption>) -> BootstrapData {
        BootstrapData {
            provider_options,
            accounts: self.accounts.clone(),
            folders: self.folders.clone(),
            messages: self.messages_for_folder(&self.selected_folder_id),
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

        if self.accounts.iter().any(|other| other.id == account.id) {
            return Err(format!(
                "Account already connected: {}",
                account.email_address
            ));
        }

        let account_id = account.id.clone();
        let account_email = account.email_address.clone();
        let provider = sync_state.provider;

        self.accounts.push(account);
        self.folders.extend(folders);
        self.messages.extend(messages);
        self.credentials.push(credential);
        self.save_sync_state(sync_state)?;
        self.enqueue_outbox(NewOutboxItem {
            account_id: account_id.clone(),
            provider,
            operation: "account.connected".into(),
            payload_json: format!(
                "{{\"accountId\":\"{account_id}\",\"emailAddress\":\"{account_email}\"}}"
            ),
        })?;
        self.recalculate_unread();

        if self.inbox_folder_id(&account_id).is_some() {
            self.selected_folder_id = UNIFIED_INBOX_ID.into();
            self.selected_message_id = self
                .messages_for_folder(&self.selected_folder_id)
                .first()
                .map(|message| message.id.clone());
        }

        self.set_sync_status_from_outbox();
        Ok(self.bootstrap(provider_options))
    }

    fn account_ids(&self) -> Vec<String> {
        self.accounts
            .iter()
            .map(|account| account.id.clone())
            .collect()
    }

    fn account_by_provider_id(
        &self,
        provider: ProviderKind,
        provider_account_id: &str,
    ) -> Option<Account> {
        self.sync_states
            .iter()
            .find(|state| {
                state.provider == provider && state.remote_account_id == provider_account_id
            })
            .and_then(|state| {
                self.accounts
                    .iter()
                    .find(|account| account.id == state.account_id)
            })
            .cloned()
    }

    fn set_account_connected(&mut self, account_id: &str, connected: bool) -> Result<(), String> {
        let account = self
            .accounts
            .iter_mut()
            .find(|account| account.id == account_id)
            .ok_or_else(|| format!("Unknown account: {account_id}"))?;
        account.connected = connected;
        Ok(())
    }

    fn remove_account(&mut self, account_id: &str) -> Result<(), String> {
        if !self.accounts.iter().any(|other| other.id == account_id) {
            return Err(format!("Unknown account: {account_id}"));
        }

        self.accounts.retain(|account| account.id != account_id);
        self.folders
            .retain(|folder| folder.account_id != account_id);
        self.messages
            .retain(|message| message.account_id != account_id);
        self.credentials
            .retain(|credential| credential.account_id != account_id);
        self.sync_states
            .retain(|state| state.account_id != account_id);
        // next_outbox_id is never rolled back, so ids stay unique across removals.
        self.outbox.retain(|item| item.account_id != account_id);
        self.selected_message_id = None;
        self.selected_folder_id = if self.accounts.is_empty() {
            String::new()
        } else {
            UNIFIED_INBOX_ID.into()
        };
        self.set_sync_status_from_outbox();
        Ok(())
    }

    fn credential_ref(&self, account_id: &str) -> Option<String> {
        self.sync_states
            .iter()
            .find(|state| state.account_id == account_id)
            .map(|state| format!("{}:{}", state.provider.slug(), state.remote_account_id))
    }

    fn select_folder(&mut self, folder_id: &str) -> Result<MailboxSnapshot, String> {
        if folder_id != UNIFIED_INBOX_ID && !self.folders.iter().any(|f| f.id == folder_id) {
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

    fn search(&mut self, query: &str) -> MailboxSnapshot {
        let normalized = query.trim().to_lowercase();
        let matches = if normalized.is_empty() {
            self.messages_for_folder(&self.selected_folder_id)
        } else {
            let mut found: Vec<MessageSummary> = self
                .messages
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
                .collect();
            found.sort_by(|left, right| right.received_at.cmp(&left.received_at));
            found
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

    /// Archive removes inbox membership. It does not overwrite the folder set:
    /// Gmail's Archive is the absence of `INBOX`, not a label of its own.
    fn archive_message(&mut self, message_id: &str) -> Result<MailboxSnapshot, String> {
        let previous_folder = self.selected_folder_id.clone();
        let account_id = self
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .map(|message| message.account_id.clone())
            .ok_or_else(|| format!("Unknown message: {message_id}"))?;

        let inbox_ids: Vec<String> = self
            .folders
            .iter()
            .filter(|folder| folder.account_id == account_id && folder.role == "inbox")
            .map(|folder| folder.id.clone())
            .collect();

        let archive_folder_id = self
            .folders
            .iter()
            .find(|folder| folder.account_id == account_id && folder.role == "archive")
            .map(|folder| folder.id.clone());

        let message = self
            .messages
            .iter_mut()
            .find(|message| message.id == message_id)
            .ok_or_else(|| format!("Unknown message: {message_id}"))?;

        message
            .folder_ids
            .retain(|folder_id| !inbox_ids.contains(folder_id));
        if let Some(archive_id) = archive_folder_id {
            if !message.folder_ids.contains(&archive_id) {
                message.folder_ids.push(archive_id);
            }
        }
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

    fn refresh(&mut self, provider_options: Vec<ProviderOption>) -> BootstrapData {
        self.set_sync_status_from_outbox();
        self.bootstrap(provider_options)
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
            .position(|other| other.account_id == state.account_id)
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
            match self.folders.iter().position(|other| other.id == folder.id) {
                Some(index) => {
                    let unread = self.folders[index].unread_count;
                    self.folders[index] = folder;
                    self.folders[index].unread_count = unread;
                }
                None => self.folders.push(folder),
            }
        }

        let message_count = delta.messages.len();
        for message in delta.messages {
            match self
                .messages
                .iter()
                .position(|other| other.id == message.id)
            {
                Some(index) => self.messages[index] = message,
                None => self.messages.push(message),
            }
        }

        self.save_sync_state(delta.sync_state)?;
        self.recalculate_unread();
        if message_count > 0 {
            self.sync_status = SyncStatus {
                state: "Synced".into(),
                last_checked: Some(now_iso8601()),
                detail: format!(
                    "{} messages cached locally; {} new from provider",
                    self.messages.len(),
                    message_count
                ),
            };
        }
        Ok(())
    }

    fn message_count(&self, account_id: &str) -> usize {
        self.messages
            .iter()
            .filter(|message| message.account_id == account_id)
            .count()
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
            created_at: now_iso8601(),
            last_attempted_at: None,
            last_synced_at: None,
            last_error: None,
            next_attempt_at: None,
            failure_kind: None,
        };
        self.next_outbox_id += 1;
        self.outbox.push(outbox_item.clone());
        self.set_sync_status_from_outbox();
        Ok(outbox_item)
    }

    fn pending_outbox(&self) -> Vec<SyncOutboxItem> {
        let now = now_iso8601();
        self.outbox
            .iter()
            .filter(|item| match item.status.as_str() {
                "pending" => true,
                "retryable" => item
                    .next_attempt_at
                    .as_deref()
                    .map(|at| at <= now.as_str())
                    .unwrap_or(true),
                _ => false,
            })
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
                "pending" | "retryable" => summary.pending += 1,
                "synced" => summary.synced += 1,
                "failed" => summary.failed += 1,
                _ => {}
            }
        }

        summary
    }

    fn mark_outbox_synced(&mut self, id: &str) -> Result<(), String> {
        let now = now_iso8601();
        let item = self
            .outbox
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| format!("Unknown outbox item: {id}"))?;

        item.status = "synced".into();
        item.attempt_count += 1;
        item.last_attempted_at = Some(now.clone());
        item.last_synced_at = Some(now);
        item.last_error = None;
        item.next_attempt_at = None;
        item.failure_kind = None;
        self.set_sync_status_from_outbox();
        Ok(())
    }

    /// Retryable with a backoff until the attempt cap, then parked. Writing
    /// `failed` on the first attempt would strand the item: `pending_outbox`
    /// selects only pending and retryable.
    fn mark_outbox_failed(&mut self, id: &str, error: String) -> Result<(), String> {
        let now = now_iso8601();
        let item = self
            .outbox
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| format!("Unknown outbox item: {id}"))?;

        item.attempt_count += 1;
        let parked = item.attempt_count as i64 >= OUTBOX_ATTEMPT_CAP;
        item.status = if parked { "failed" } else { "retryable" }.into();
        item.next_attempt_at = if parked {
            None
        } else {
            Some(retry_at(item.attempt_count as i64))
        };
        item.failure_kind = Some(if parked { "permanent" } else { "transient" }.into());
        item.last_attempted_at = Some(now);
        item.last_error = Some(error);
        self.set_sync_status_from_outbox();
        Ok(())
    }

    fn set_sync_status(&mut self, status: SyncStatus) {
        self.sync_status = status;
    }

    fn sync_status(&self) -> SyncStatus {
        self.sync_status.clone()
    }
}
