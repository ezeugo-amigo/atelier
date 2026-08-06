//! The storage boundary. `MailStore` is an in-memory implementation kept for
//! fast unit tests; `SqliteMailStorage` is what ships.

// The in-memory store exists so provider and sync-engine tests need no
// filesystem. It is not compiled into the shipped binary.
#[cfg(test)]
pub mod memory;
pub mod sqlite;

use crate::model::*;

#[allow(dead_code)] // account_ids / set_account_connected / set_sync_status are
                    // the Phase 4-5 reconnect and progress surface; they land with the callers.
pub trait MailStorage: Send {
    fn bootstrap(&self, provider_options: Vec<ProviderOption>) -> BootstrapData;
    fn add_account(
        &mut self,
        seed: AccountSeed,
        provider_options: Vec<ProviderOption>,
    ) -> Result<BootstrapData, String>;
    fn account_ids(&self) -> Vec<String>;
    fn account_by_provider_id(
        &self,
        provider: ProviderKind,
        provider_account_id: &str,
    ) -> Option<Account>;
    fn set_account_connected(&mut self, account_id: &str, connected: bool) -> Result<(), String>;
    fn remove_account(&mut self, account_id: &str) -> Result<(), String>;
    fn credential_ref(&self, account_id: &str) -> Option<String>;
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
    fn message_count(&self, account_id: &str) -> usize;
    fn enqueue_outbox(&mut self, item: NewOutboxItem) -> Result<SyncOutboxItem, String>;
    fn pending_outbox(&self) -> Vec<SyncOutboxItem>;
    fn outbox_entries(&self) -> Vec<SyncOutboxItem>;
    fn outbox_summary(&self) -> SyncOutboxSummary;
    fn mark_outbox_synced(&mut self, id: &str) -> Result<(), String>;
    fn mark_outbox_failed(&mut self, id: &str, error: String) -> Result<(), String>;
    fn set_sync_status(&mut self, status: SyncStatus);
    fn sync_status(&self) -> SyncStatus;
}
