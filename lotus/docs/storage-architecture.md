# Lotus Storage Architecture

Lotus treats providers as import/sync adapters, not as owners of mailbox state.
The canonical email state belongs to the storage layer.

```text
Tauri commands
  -> MailStorage
    -> connected accounts
    -> credential references
    -> provider sync state
    -> normalized folders/messages
    -> local sync outbox
  -> MailProvider registry
    -> provider auth and remote API quirks
  -> SyncEngine
    -> imports provider mailbox deltas into local storage
    -> drains local mutations when connectivity is available
```

The current implementation is `MailStore`, an in-memory `MailStorage`
implementation. The intended durable implementation is SQLite plus OS keychain
credential storage. The SQL shape lives in:

```text
src-tauri/migrations/001_mail_storage.sql
```

## Interface Boundary

`MailStorage` owns the local source of truth:

- account records and provider identity
- credential references, not raw secrets
- current provider sync cursors and watermarks
- normalized folders, messages, labels, and folder membership
- local read/archive/search projections
- the offline-readable mailbox cache
- local mutation outbox for writes that still need remote/server sync

`MailProvider` owns provider-specific work:

- login/setup flow
- remote identifiers and pagination rules
- API-specific cursors such as Gmail history IDs or Microsoft delta tokens
- conversion from provider payloads into Lotus-normalized records

Providers should return account seeds or mailbox sync deltas. Storage applies
them to the normalized local cache and updates `provider_sync_state`.

`SyncEngine` owns import, retry, and delivery:

- reads provider sync states from storage
- asks each provider for the next mailbox delta
- applies new folders/messages to local storage before the UI reads them
- reads pending `sync_outbox` items from storage
- sends them to the server or provider-specific remote API
- marks items as `synced` or `failed`
- leaves failed/pending work local so the app remains usable offline

The current Tauri surface exposes this checkpoint through
`provider_sync_checkpoint(accountId)`. That command is primarily for debugging
and future sync workers; the Elm dashboard still renders from normalized
mailbox snapshots.

The current Tauri surface also exposes:

- `sync_pending_changes()` to run one sync pass
- `sync_outbox_status()` to inspect pending/synced/failed counts
- `sync_outbox_entries()` to inspect local queued mutations

`refresh_mail` opportunistically runs one sync pass before returning the latest
local dashboard snapshot.

## Core Tables

`connected_accounts` stores one local account per provider identity. The
`credential_ref` points to future keychain storage; refresh tokens should not be
stored directly in SQLite.

`provider_sync_state` stores where Lotus is in a provider mailbox stream:

- `sync_cursor` for generic provider cursors
- `history_id` for Gmail-style history replay
- `delta_token` for Microsoft Graph-style delta sync
- `page_token` for interrupted page walks
- `high_watermark` for fallback date or internal-date checkpoints
- status/error timestamps for retry and UI reporting

`mail_folders`, `mail_messages`, `mail_message_folders`, and
`mail_message_labels` store the normalized mailbox projection used by Elm.
That projection is the offline inbox cache. Once a message has been imported,
the dashboard can render it without a provider connection.

`sync_outbox` is the local-first write queue. Local actions update the mailbox
immediately and append an outbox row. The sync engine later marks rows as
`synced` after remote delivery or `failed` after a retryable error.

`provider_sync_events` is append-only operational history for import/sync runs.
It is useful for debugging large syncs without overloading
`provider_sync_state`, which should only contain the latest checkpoint.

## Inbound Sync

Inbound sync is provider-to-storage:

```text
provider_sync_state -> MailProvider.sync_mailbox -> MailboxDelta -> MailStorage.apply_mailbox_delta
```

The provider never owns the inbox after conversion. It only knows how to fetch
the next page, history batch, or delta response. Storage upserts the returned
folders/messages and advances the checkpoint in one boundary.

If a provider fetch fails, the sync engine records the failure on
`provider_sync_state` and keeps serving the existing local cache. Offline mode
should degrade to stale-but-readable mail, not an empty or blocked dashboard.

With SQLite, swapping providers does not change the Elm UI. Swapping SQLite for
another durable store should only require a new `MailStorage` implementation
that preserves the same local cache and checkpoint semantics.
