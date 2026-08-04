//! The local-first sync engine.
//!
//! Ordering is load-bearing. The outbox drains to completion before any inbound
//! delta is applied, because an inbound delta carries the provider's view of
//! label state and would otherwise replay the old value over a still-pending
//! optimistic local change. The user marks a message read, refreshes, and
//! watches it flip back.

use tokio::sync::Mutex;

use crate::model::*;
use crate::provider::ProviderRegistry;
use crate::storage::MailStorage;

pub struct LocalFirstSyncEngine;

impl LocalFirstSyncEngine {
    /// Read state under the lock, drop it, await the provider, re-take it to
    /// apply. The lock is never held across an `.await`.
    pub async fn sync_once<S: MailStorage + ?Sized>(
        &self,
        storage: &Mutex<Box<S>>,
        providers: &ProviderRegistry,
    ) -> Result<SyncReport, String> {
        let (pending, sync_states) = {
            let guard = storage.lock().await;
            (guard.pending_outbox(), guard.sync_states())
        };

        let attempted = pending.len();
        let mut synced = 0usize;
        let mut failed = 0usize;
        let mut inbound_messages = 0usize;

        // Drain the outbox first. Phase 5 replaces the no-op dispatch below with
        // real provider calls; the ordering guarantee is what matters here.
        for item in pending {
            let mut guard = storage.lock().await;
            match guard.mark_outbox_synced(&item.id) {
                Ok(()) => synced += 1,
                Err(error) => {
                    failed += 1;
                    let _ = guard.mark_outbox_failed(&item.id, error);
                }
            }
        }

        for sync_state in sync_states {
            let provider = match providers.get(sync_state.provider) {
                Ok(provider) => provider,
                Err(error) => {
                    let mut guard = storage.lock().await;
                    guard.save_sync_state(failed_sync_state(sync_state, error))?;
                    failed += 1;
                    continue;
                }
            };

            let result = provider.sync_mailbox(&sync_state).await;

            let mut guard = storage.lock().await;
            match result {
                Ok(delta) => {
                    inbound_messages += delta.messages.len();
                    guard.apply_mailbox_delta(delta)?;
                }
                Err(error) => {
                    guard.save_sync_state(failed_sync_state(sync_state, error))?;
                    failed += 1;
                }
            }
        }

        let remaining_pending = storage.lock().await.outbox_summary().pending;
        Ok(SyncReport {
            attempted,
            synced,
            failed,
            inbound_messages,
            remaining_pending,
        })
    }
}
