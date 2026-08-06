//! Durable mailbox storage on SQLite.
//!
//! Reads go through the same shapes the in-memory store produces, so command
//! responses are identical either way. Writes are transactional per unit of
//! work: a folder-membership rewrite never leaves a message half-filed.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::model::*;
use crate::storage::MailStorage;

const SCHEMA_001: &str = include_str!("../../migrations/001_mail_storage.sql");
const CURRENT_SCHEMA_VERSION: i64 = 1;

pub struct SqliteMailStorage {
    connection: Connection,
    selected_folder_id: String,
    selected_message_id: Option<String>,
    sync_status: SyncStatus,
}

impl SqliteMailStorage {
    pub fn open(path: &Path) -> Result<Self, String> {
        let connection = Connection::open(path)
            .map_err(|error| format!("Could not open the mailbox database: {error}"))?;
        Self::from_connection(connection)
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self, String> {
        let connection = Connection::open_in_memory()
            .map_err(|error| format!("Could not open an in-memory database: {error}"))?;
        Self::from_connection(connection)
    }

    fn from_connection(connection: Connection) -> Result<Self, String> {
        let mut storage = Self {
            connection,
            selected_folder_id: String::new(),
            selected_message_id: None,
            sync_status: SyncStatus {
                state: "Ready".into(),
                last_checked: None,
                detail: "No account connected".into(),
            },
        };
        storage.migrate()?;
        storage.restore_selection();
        storage.refresh_status_from_outbox();
        Ok(storage)
    }

    /// Migration failure is fatal, with the reason in the message. A partially
    /// migrated mailbox is worse than a refusal to start.
    fn migrate(&mut self) -> Result<(), String> {
        self.connection
            .execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .map_err(|error| format!("Could not configure the mailbox database: {error}"))?;

        // `schema_version` may not exist yet on a first run, so ask
        // sqlite_master whether the table is there before querying it.
        let has_version_table = self
            .connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_version'",
                [],
                |_| Ok(()),
            )
            .optional()
            .map_err(|error| format!("Could not inspect the mailbox database: {error}"))?
            .is_some();

        let applied = if has_version_table {
            self.connection
                .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                    row.get::<_, Option<i64>>(0)
                })
                .map_err(|error| format!("Could not read the schema version: {error}"))?
                .unwrap_or(0)
        } else {
            0
        };

        if applied >= CURRENT_SCHEMA_VERSION {
            return Ok(());
        }

        self.connection.execute_batch(SCHEMA_001).map_err(|error| {
            format!("Mailbox database migration 001 failed and Lotus cannot start: {error}")
        })?;

        self.connection
            .execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![CURRENT_SCHEMA_VERSION],
            )
            .map_err(|error| format!("Could not record the schema version: {error}"))?;

        Ok(())
    }

    fn restore_selection(&mut self) {
        let has_account = self
            .connection
            .query_row("SELECT COUNT(*) FROM connected_accounts", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap_or(0)
            > 0;

        if has_account {
            self.selected_folder_id = UNIFIED_INBOX_ID.into();
            self.selected_message_id = self
                .messages_for_folder(UNIFIED_INBOX_ID)
                .first()
                .map(|message| message.id.clone());
        }
    }

    fn accounts(&self) -> Vec<Account> {
        self.connection
            .prepare(
                "SELECT id, display_name, email_address, provider_label, provider, accent, connected
                 FROM connected_accounts ORDER BY created_at",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| {
                        let slug: String = row.get(4)?;
                        Ok(Account {
                            id: row.get(0)?,
                            display_name: row.get(1)?,
                            email_address: row.get(2)?,
                            provider: row.get(3)?,
                            provider_kind: ProviderKind::from_slug(&slug)
                                .unwrap_or(ProviderKind::MockGmail),
                            accent: row.get(5)?,
                            connected: row.get::<_, i64>(6)? == 1,
                        })
                    })
                    .and_then(|rows| rows.collect())
            })
            .unwrap_or_default()
    }

    fn folders(&self) -> Vec<Folder> {
        self.connection
            .prepare(
                "SELECT id, account_id, name, role, provider_folder_id, unread_count
                 FROM mail_folders ORDER BY account_id, rowid",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| {
                        Ok(Folder {
                            id: row.get(0)?,
                            account_id: row.get(1)?,
                            name: row.get(2)?,
                            role: row.get(3)?,
                            provider_folder_id: row.get(4)?,
                            unread_count: row.get::<_, i64>(5)? as usize,
                        })
                    })
                    .and_then(|rows| rows.collect())
            })
            .unwrap_or_default()
    }

    fn message_row(&self, row: &Row<'_>) -> rusqlite::Result<MessageDetail> {
        let id: String = row.get(0)?;
        Ok(MessageDetail {
            id: id.clone(),
            account_id: row.get(1)?,
            folder_ids: Vec::new(),
            sender_name: row.get(2)?,
            sender_email: row.get(3)?,
            subject: row.get(4)?,
            snippet: row.get(5)?,
            received_at: row.get(6)?,
            internal_date: row.get(7)?,
            unread: row.get::<_, i64>(8)? == 1,
            starred: row.get::<_, i64>(9)? == 1,
            labels: Vec::new(),
            to: decode_string_list(&row.get::<_, String>(10)?),
            cc: decode_string_list(&row.get::<_, String>(11)?),
            reply_to: decode_string_list(&row.get::<_, String>(12)?),
            body_paragraphs: decode_string_list(&row.get::<_, String>(13)?),
            provider_message_id: row.get(14)?,
            provider_thread_id: row.get(15)?,
        })
    }

    const MESSAGE_COLUMNS: &'static str = "id, account_id, sender_name, sender_email, subject, \
         snippet, received_at, internal_date, unread, starred, to_json, cc_json, reply_to_json, \
         body_json, provider_message_id, provider_thread_id";

    fn hydrate(&self, mut message: MessageDetail) -> MessageDetail {
        message.folder_ids = self
            .connection
            .prepare("SELECT folder_id FROM mail_message_folders WHERE message_id = ?1")
            .and_then(|mut statement| {
                statement
                    .query_map(params![message.id], |row| row.get::<_, String>(0))
                    .and_then(|rows| rows.collect())
            })
            .unwrap_or_default();

        message.labels = self
            .connection
            .prepare("SELECT label FROM mail_message_labels WHERE message_id = ?1 ORDER BY label")
            .and_then(|mut statement| {
                statement
                    .query_map(params![message.id], |row| row.get::<_, String>(0))
                    .and_then(|rows| rows.collect())
            })
            .unwrap_or_default();

        message
    }

    fn message_by_id(&self, message_id: &str) -> Result<MessageDetail, String> {
        let sql = format!(
            "SELECT {} FROM mail_messages WHERE id = ?1 AND deleted_at IS NULL",
            Self::MESSAGE_COLUMNS
        );
        self.connection
            .query_row(&sql, params![message_id], |row| self.message_row(row))
            .optional()
            .map_err(|error| error.to_string())?
            .map(|message| self.hydrate(message))
            .ok_or_else(|| format!("Unknown message: {message_id}"))
    }

    /// Folder membership through `mail_message_folders`. The `IN (...)` over
    /// inbox folder ids yields each message once, so the unified inbox cannot
    /// show duplicates for a message carrying two inbox-role labels.
    pub fn messages_for_folder(&self, folder_id: &str) -> Vec<MessageSummary> {
        let details = if folder_id == UNIFIED_INBOX_ID {
            let sql = format!(
                "SELECT {} FROM mail_messages m
                 WHERE m.deleted_at IS NULL AND EXISTS (
                   SELECT 1 FROM mail_message_folders mf
                   JOIN mail_folders f ON f.id = mf.folder_id
                   WHERE mf.message_id = m.id AND f.role = 'inbox'
                 )
                 ORDER BY m.internal_date DESC",
                prefixed_columns()
            );
            self.query_messages(&sql, params![])
        } else {
            let folder: Option<(String, String)> = self
                .connection
                .query_row(
                    "SELECT role, account_id FROM mail_folders WHERE id = ?1",
                    params![folder_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .ok()
                .flatten();

            let (role, account_id) = folder.unwrap_or_else(|| ("inbox".into(), String::new()));

            if role == "starred" {
                // Starred reads the flag rather than a folder membership, so the
                // account filter has to be explicit. Without it, every connected
                // account's Starred folder shows every other account's mail.
                let sql = format!(
                    "SELECT {} FROM mail_messages m
                     WHERE m.deleted_at IS NULL AND m.starred = 1 AND m.account_id = ?1
                     ORDER BY m.internal_date DESC",
                    prefixed_columns()
                );
                self.query_messages(&sql, params![account_id])
            } else {
                let sql = format!(
                    "SELECT {} FROM mail_messages m
                     JOIN mail_message_folders mf ON mf.message_id = m.id
                     WHERE m.deleted_at IS NULL AND mf.folder_id = ?1
                     ORDER BY m.internal_date DESC",
                    prefixed_columns()
                );
                self.query_messages(&sql, params![folder_id])
            }
        };

        details.into_iter().map(|m| m.summary()).collect()
    }

    fn query_messages(&self, sql: &str, parameters: impl rusqlite::Params) -> Vec<MessageDetail> {
        self.connection
            .prepare(sql)
            .and_then(|mut statement| {
                statement
                    .query_map(parameters, |row| self.message_row(row))
                    .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            })
            .map(|messages| {
                messages
                    .into_iter()
                    .map(|message| self.hydrate(message))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn selected_message(&self) -> Option<MessageDetail> {
        self.selected_message_id
            .as_ref()
            .and_then(|id| self.message_by_id(id).ok())
    }

    fn snapshot(&self, folder_id: &str) -> MailboxSnapshot {
        MailboxSnapshot {
            folder_id: folder_id.into(),
            folders: self.folders(),
            messages: self.messages_for_folder(folder_id),
            selected_message_id: self.selected_message_id.clone(),
            selected_message: self.selected_message(),
            sync_status: self.sync_status.clone(),
        }
    }

    fn write_message(
        transaction: &rusqlite::Transaction<'_>,
        message: &MessageDetail,
    ) -> Result<(), String> {
        transaction
            .execute(
                "INSERT INTO mail_messages (
                     id, account_id, provider_message_id, provider_thread_id, sender_name,
                     sender_email, subject, snippet, received_at, internal_date, unread, starred,
                     to_json, cc_json, reply_to_json, body_json, body_text
                 ) VALUES (
                     ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17
                 )
                 ON CONFLICT(id) DO UPDATE SET
                     provider_thread_id = excluded.provider_thread_id,
                     sender_name = excluded.sender_name,
                     sender_email = excluded.sender_email,
                     subject = excluded.subject,
                     snippet = excluded.snippet,
                     received_at = excluded.received_at,
                     internal_date = excluded.internal_date,
                     unread = excluded.unread,
                     starred = excluded.starred,
                     to_json = excluded.to_json,
                     cc_json = excluded.cc_json,
                     reply_to_json = excluded.reply_to_json,
                     body_json = excluded.body_json,
                     body_text = excluded.body_text,
                     deleted_at = NULL,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![
                    message.id,
                    message.account_id,
                    message
                        .provider_message_id
                        .clone()
                        .unwrap_or_else(|| message.id.clone()),
                    message.provider_thread_id,
                    message.sender_name,
                    message.sender_email,
                    message.subject,
                    message.snippet,
                    message.received_at,
                    message.internal_date,
                    i64::from(message.unread),
                    i64::from(message.starred),
                    encode_string_list(&message.to),
                    encode_string_list(&message.cc),
                    encode_string_list(&message.reply_to),
                    encode_string_list(&message.body_paragraphs),
                    message.body_paragraphs.join("\n\n"),
                ],
            )
            .map_err(|error| format!("Could not save message {}: {error}", message.id))?;

        transaction
            .execute(
                "DELETE FROM mail_message_folders WHERE message_id = ?1",
                params![message.id],
            )
            .map_err(|error| error.to_string())?;
        for folder_id in &message.folder_ids {
            transaction
                .execute(
                    "INSERT OR IGNORE INTO mail_message_folders (message_id, folder_id)
                     VALUES (?1, ?2)",
                    params![message.id, folder_id],
                )
                .map_err(|error| error.to_string())?;
        }

        transaction
            .execute(
                "DELETE FROM mail_message_labels WHERE message_id = ?1",
                params![message.id],
            )
            .map_err(|error| error.to_string())?;
        for label in &message.labels {
            transaction
                .execute(
                    "INSERT OR IGNORE INTO mail_message_labels (message_id, label)
                     VALUES (?1, ?2)",
                    params![message.id, label],
                )
                .map_err(|error| error.to_string())?;
        }

        Ok(())
    }

    fn write_folder(
        transaction: &rusqlite::Transaction<'_>,
        folder: &Folder,
    ) -> Result<(), String> {
        transaction
            .execute(
                "INSERT INTO mail_folders (
                     id, account_id, provider_folder_id, name, role, unread_count
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                     name = excluded.name,
                     role = excluded.role,
                     provider_folder_id = excluded.provider_folder_id,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![
                    folder.id,
                    folder.account_id,
                    folder.provider_folder_id,
                    folder.name,
                    folder.role,
                    folder.unread_count as i64,
                ],
            )
            .map_err(|error| format!("Could not save folder {}: {error}", folder.id))?;
        Ok(())
    }

    fn recalculate_unread(&self) -> Result<(), String> {
        self.connection
            .execute_batch(
                "UPDATE mail_folders SET unread_count = (
                   SELECT COUNT(*) FROM mail_messages m
                   WHERE m.deleted_at IS NULL AND m.unread = 1
                     AND CASE WHEN mail_folders.role = 'starred'
                              THEN m.starred = 1 AND m.account_id = mail_folders.account_id
                              ELSE EXISTS (
                                SELECT 1 FROM mail_message_folders mf
                                WHERE mf.message_id = m.id AND mf.folder_id = mail_folders.id
                              )
                         END
                 );",
            )
            .map_err(|error| format!("Could not recalculate unread counts: {error}"))
    }

    fn refresh_status_from_outbox(&mut self) {
        let summary = self.outbox_summary();
        let cached = self.total_messages();
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
        } else if cached == 0 {
            SyncStatus {
                state: "Ready".into(),
                last_checked: None,
                detail: "No account connected".into(),
            }
        } else {
            SyncStatus {
                state: "Synced".into(),
                last_checked: Some(now_iso8601()),
                detail: format!("{cached} messages cached locally"),
            }
        };
    }

    fn total_messages(&self) -> usize {
        self.connection
            .query_row(
                "SELECT COUNT(*) FROM mail_messages WHERE deleted_at IS NULL",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0) as usize
    }

    fn account_exists(&self, account_id: &str) -> bool {
        self.connection
            .query_row(
                "SELECT 1 FROM connected_accounts WHERE id = ?1",
                params![account_id],
                |_| Ok(()),
            )
            .optional()
            .unwrap_or(None)
            .is_some()
    }

    fn queue_message_mutation(
        &mut self,
        message_id: &str,
        operation: &str,
        payload_json: String,
    ) -> Result<(), String> {
        let account_id = self.message_by_id(message_id)?.account_id;
        let provider = self
            .sync_state(&account_id)
            .map(|state| state.provider)
            .ok_or_else(|| format!("No provider sync state for account: {account_id}"))?;
        self.enqueue_outbox(NewOutboxItem {
            account_id,
            provider,
            operation: operation.into(),
            payload_json,
        })?;
        Ok(())
    }
}

impl MailStorage for SqliteMailStorage {
    fn bootstrap(&self, provider_options: Vec<ProviderOption>) -> BootstrapData {
        BootstrapData {
            provider_options,
            accounts: self.accounts(),
            folders: self.folders(),
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

        if self.account_exists(&account.id) {
            return Err(format!(
                "Account already connected: {}",
                account.email_address
            ));
        }

        let account_id = account.id.clone();
        let account_email = account.email_address.clone();
        let provider = sync_state.provider;
        let credential_ref = format!("{}:{}", provider.slug(), sync_state.remote_account_id);

        let transaction = self
            .connection
            .transaction()
            .map_err(|error| error.to_string())?;

        transaction
            .execute(
                "INSERT INTO connected_accounts (
                     id, provider, provider_account_id, display_name, provider_label,
                     email_address, credential_ref, accent, connected
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    account.id,
                    provider.slug(),
                    sync_state.remote_account_id,
                    account.display_name,
                    account.provider,
                    account.email_address,
                    credential_ref,
                    account.accent,
                    i64::from(account.connected),
                ],
            )
            .map_err(|error| format!("Could not save the account: {error}"))?;

        for folder in &folders {
            SqliteMailStorage::write_folder(&transaction, folder)?;
        }
        for message in &messages {
            SqliteMailStorage::write_message(&transaction, message)?;
        }

        transaction
            .execute(
                "INSERT INTO provider_sync_state (
                     account_id, provider, remote_account_id, sync_cursor, history_id,
                     delta_token, page_token, high_watermark, status, last_attempted_at,
                     last_successful_at, last_error
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    sync_state.account_id,
                    sync_state.provider.slug(),
                    sync_state.remote_account_id,
                    sync_state.sync_cursor,
                    sync_state.history_id,
                    sync_state.delta_token,
                    sync_state.page_token,
                    sync_state.high_watermark,
                    sync_state.status,
                    sync_state.last_attempted_at,
                    sync_state.last_successful_at,
                    sync_state.last_error,
                ],
            )
            .map_err(|error| format!("Could not save the sync checkpoint: {error}"))?;

        transaction
            .commit()
            .map_err(|error| format!("Could not commit the new account: {error}"))?;

        // The credential value itself never reaches SQLite. Only the keychain
        // reference above is persisted.
        drop(credential);

        self.enqueue_outbox(NewOutboxItem {
            account_id: account_id.clone(),
            provider,
            operation: "account.connected".into(),
            payload_json: format!(
                "{{\"accountId\":\"{account_id}\",\"emailAddress\":\"{account_email}\"}}"
            ),
        })?;
        self.recalculate_unread()?;

        self.selected_folder_id = UNIFIED_INBOX_ID.into();
        self.selected_message_id = self
            .messages_for_folder(UNIFIED_INBOX_ID)
            .first()
            .map(|message| message.id.clone());
        self.refresh_status_from_outbox();

        Ok(self.bootstrap(provider_options))
    }

    fn account_ids(&self) -> Vec<String> {
        self.accounts().into_iter().map(|a| a.id).collect()
    }

    fn account_by_provider_id(
        &self,
        provider: ProviderKind,
        provider_account_id: &str,
    ) -> Option<Account> {
        let id: Option<String> = self
            .connection
            .query_row(
                "SELECT id FROM connected_accounts
                 WHERE provider = ?1 AND provider_account_id = ?2",
                params![provider.slug(), provider_account_id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten();

        id.and_then(|id| self.accounts().into_iter().find(|a| a.id == id))
    }

    fn set_account_connected(&mut self, account_id: &str, connected: bool) -> Result<(), String> {
        let changed = self
            .connection
            .execute(
                "UPDATE connected_accounts SET connected = ?2,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1",
                params![account_id, i64::from(connected)],
            )
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err(format!("Unknown account: {account_id}"));
        }
        Ok(())
    }

    fn remove_account(&mut self, account_id: &str) -> Result<(), String> {
        let removed = self
            .connection
            .execute(
                "DELETE FROM connected_accounts WHERE id = ?1",
                params![account_id],
            )
            .map_err(|error| error.to_string())?;
        if removed == 0 {
            return Err(format!("Unknown account: {account_id}"));
        }

        self.selected_message_id = None;
        self.selected_folder_id = if self.accounts().is_empty() {
            String::new()
        } else {
            UNIFIED_INBOX_ID.into()
        };
        self.refresh_status_from_outbox();
        Ok(())
    }

    fn credential_ref(&self, account_id: &str) -> Option<String> {
        self.connection
            .query_row(
                "SELECT credential_ref FROM connected_accounts WHERE id = ?1",
                params![account_id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    fn select_folder(&mut self, folder_id: &str) -> Result<MailboxSnapshot, String> {
        if folder_id != UNIFIED_INBOX_ID
            && self
                .connection
                .query_row(
                    "SELECT 1 FROM mail_folders WHERE id = ?1",
                    params![folder_id],
                    |_| Ok(()),
                )
                .optional()
                .map_err(|error| error.to_string())?
                .is_none()
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
        self.connection
            .execute(
                "UPDATE mail_messages SET unread = 0 WHERE id = ?1",
                params![message_id],
            )
            .map_err(|error| error.to_string())?;
        self.recalculate_unread()?;

        if was_unread {
            self.queue_message_mutation(
                message_id,
                "message.mark_read",
                format!("{{\"messageId\":\"{message_id}\",\"read\":true}}"),
            )?;
        }

        self.selected_message_id = Some(message_id.into());
        Ok(MessageUpdate {
            folders: self.folders(),
            message: self.message_by_id(message_id)?,
            sync_status: self.sync_status.clone(),
        })
    }

    /// Naive substring search over header fields, matching the in-memory store.
    /// At the Phase 3 cap of 5000 messages this is milliseconds, and search is
    /// user-triggered rather than fired per keystroke. FTS5 belongs with a
    /// larger cap.
    fn search(&mut self, query: &str) -> MailboxSnapshot {
        let normalized = query.trim().to_lowercase();
        if normalized.is_empty() {
            let folder_id = self.selected_folder_id.clone();
            return self.snapshot(&folder_id);
        }

        let pattern = format!("%{normalized}%");
        let sql = format!(
            "SELECT {} FROM mail_messages m
             WHERE m.deleted_at IS NULL AND (
               lower(m.sender_name) LIKE ?1 OR lower(m.sender_email) LIKE ?1
               OR lower(m.subject) LIKE ?1 OR lower(m.snippet) LIKE ?1
               OR EXISTS (
                 SELECT 1 FROM mail_message_labels ml
                 WHERE ml.message_id = m.id AND lower(ml.label) LIKE ?1
               )
             )
             ORDER BY m.internal_date DESC",
            prefixed_columns()
        );
        let messages: Vec<MessageSummary> = self
            .query_messages(&sql, params![pattern])
            .into_iter()
            .map(|message| message.summary())
            .collect();

        self.selected_message_id = messages.first().map(|message| message.id.clone());
        MailboxSnapshot {
            folder_id: "search".into(),
            folders: self.folders(),
            messages,
            selected_message_id: self.selected_message_id.clone(),
            selected_message: self.selected_message(),
            sync_status: self.sync_status.clone(),
        }
    }

    fn mark_message_read(&mut self, message_id: &str, read: bool) -> Result<MessageUpdate, String> {
        let was_read = !self.message_by_id(message_id)?.unread;
        self.connection
            .execute(
                "UPDATE mail_messages SET unread = ?2 WHERE id = ?1",
                params![message_id, i64::from(!read)],
            )
            .map_err(|error| error.to_string())?;
        self.recalculate_unread()?;

        if was_read != read {
            self.queue_message_mutation(
                message_id,
                "message.mark_read",
                format!("{{\"messageId\":\"{message_id}\",\"read\":{read}}}"),
            )?;
        }

        Ok(MessageUpdate {
            folders: self.folders(),
            message: self.message_by_id(message_id)?,
            sync_status: self.sync_status.clone(),
        })
    }

    fn archive_message(&mut self, message_id: &str) -> Result<MailboxSnapshot, String> {
        let previous_folder = self.selected_folder_id.clone();
        let message = self.message_by_id(message_id)?;
        let account_id = message.account_id.clone();

        let transaction = self
            .connection
            .transaction()
            .map_err(|error| error.to_string())?;

        transaction
            .execute(
                "DELETE FROM mail_message_folders
                 WHERE message_id = ?1 AND folder_id IN (
                   SELECT id FROM mail_folders WHERE account_id = ?2 AND role = 'inbox'
                 )",
                params![message_id, account_id],
            )
            .map_err(|error| error.to_string())?;

        transaction
            .execute(
                "INSERT OR IGNORE INTO mail_message_folders (message_id, folder_id)
                 SELECT ?1, id FROM mail_folders WHERE account_id = ?2 AND role = 'archive'",
                params![message_id, account_id],
            )
            .map_err(|error| error.to_string())?;

        transaction
            .execute(
                "UPDATE mail_messages SET unread = 0 WHERE id = ?1",
                params![message_id],
            )
            .map_err(|error| error.to_string())?;

        transaction
            .commit()
            .map_err(|error| format!("Could not archive the message: {error}"))?;

        self.recalculate_unread()?;
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
        self.refresh_status_from_outbox();
        self.bootstrap(provider_options)
    }

    fn sync_state(&self, account_id: &str) -> Option<ProviderSyncState> {
        self.connection
            .query_row(
                "SELECT account_id, provider, remote_account_id, sync_cursor, history_id,
                        delta_token, page_token, high_watermark, status, last_attempted_at,
                        last_successful_at, last_error
                 FROM provider_sync_state WHERE account_id = ?1",
                params![account_id],
                sync_state_from_row,
            )
            .optional()
            .ok()
            .flatten()
    }

    fn sync_states(&self) -> Vec<ProviderSyncState> {
        self.connection
            .prepare(
                "SELECT account_id, provider, remote_account_id, sync_cursor, history_id,
                        delta_token, page_token, high_watermark, status, last_attempted_at,
                        last_successful_at, last_error
                 FROM provider_sync_state ORDER BY account_id",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([], sync_state_from_row)
                    .and_then(|rows| rows.collect())
            })
            .unwrap_or_default()
    }

    fn save_sync_state(&mut self, state: ProviderSyncState) -> Result<(), String> {
        if !self.account_exists(&state.account_id) {
            return Err(format!("Unknown account: {}", state.account_id));
        }

        self.connection
            .execute(
                "INSERT INTO provider_sync_state (
                     account_id, provider, remote_account_id, sync_cursor, history_id,
                     delta_token, page_token, high_watermark, status, last_attempted_at,
                     last_successful_at, last_error
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(account_id) DO UPDATE SET
                     remote_account_id = excluded.remote_account_id,
                     sync_cursor = excluded.sync_cursor,
                     history_id = excluded.history_id,
                     delta_token = excluded.delta_token,
                     page_token = excluded.page_token,
                     high_watermark = excluded.high_watermark,
                     status = excluded.status,
                     last_attempted_at = excluded.last_attempted_at,
                     last_successful_at = excluded.last_successful_at,
                     last_error = excluded.last_error,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![
                    state.account_id,
                    state.provider.slug(),
                    state.remote_account_id,
                    state.sync_cursor,
                    state.history_id,
                    state.delta_token,
                    state.page_token,
                    state.high_watermark,
                    state.status,
                    state.last_attempted_at,
                    state.last_successful_at,
                    state.last_error,
                ],
            )
            .map_err(|error| format!("Could not save the sync checkpoint: {error}"))?;
        Ok(())
    }

    fn apply_mailbox_delta(&mut self, delta: MailboxDelta) -> Result<(), String> {
        let account_id = delta.sync_state.account_id.clone();
        if !self.account_exists(&account_id) {
            return Err(format!("Unknown account: {account_id}"));
        }

        let message_count = delta.messages.len();
        let folder_count = delta.folders.len();

        {
            let transaction = self
                .connection
                .transaction()
                .map_err(|error| error.to_string())?;
            for folder in &delta.folders {
                SqliteMailStorage::write_folder(&transaction, folder)?;
            }
            for message in &delta.messages {
                SqliteMailStorage::write_message(&transaction, message)?;
            }
            transaction
                .commit()
                .map_err(|error| format!("Could not apply the mailbox delta: {error}"))?;
        }

        self.save_sync_state(delta.sync_state)?;
        self.recalculate_unread()?;

        self.connection
            .execute(
                "INSERT INTO provider_sync_events (
                     account_id, provider, status, finished_at, messages_seen,
                     messages_upserted, folders_seen
                 ) VALUES (?1, ?2, 'ok', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), ?3, ?3, ?4)",
                params![
                    account_id,
                    self.sync_state(&account_id)
                        .map(|state| state.provider.slug())
                        .unwrap_or("gmail"),
                    message_count as i64,
                    folder_count as i64,
                ],
            )
            .map_err(|error| error.to_string())?;

        if message_count > 0 {
            let cached = self.total_messages();
            self.sync_status = SyncStatus {
                state: "Synced".into(),
                last_checked: Some(now_iso8601()),
                detail: format!(
                    "{cached} messages cached locally; {message_count} new from provider"
                ),
            };
            if self.selected_message_id.is_none() {
                self.selected_message_id = self
                    .messages_for_folder(&self.selected_folder_id)
                    .first()
                    .map(|message| message.id.clone());
            }
        }
        Ok(())
    }

    fn message_count(&self, account_id: &str) -> usize {
        self.connection
            .query_row(
                "SELECT COUNT(*) FROM mail_messages
                 WHERE account_id = ?1 AND deleted_at IS NULL",
                params![account_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0) as usize
    }

    fn enqueue_outbox(&mut self, item: NewOutboxItem) -> Result<SyncOutboxItem, String> {
        if !self.account_exists(&item.account_id) {
            return Err(format!("Unknown account: {}", item.account_id));
        }

        // AUTOINCREMENT never reissues a value, even after the rows it numbered
        // are gone. MAX(rowid) would collide after a cascade delete freed the
        // highest rowid, and `mark_outbox_synced` would then address the wrong
        // row.
        self.connection
            .execute("INSERT INTO outbox_id_sequence (id) VALUES (NULL)", [])
            .map_err(|error| format!("Could not allocate an outbox id: {error}"))?;
        let next = self.connection.last_insert_rowid();
        let id = format!("outbox-{next}");
        let created_at = now_iso8601();

        self.connection
            .execute(
                "INSERT INTO sync_outbox (
                     id, account_id, provider, operation, payload_json, status, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6)",
                params![
                    id,
                    item.account_id,
                    item.provider.slug(),
                    item.operation,
                    item.payload_json,
                    created_at,
                ],
            )
            .map_err(|error| format!("Could not queue the change: {error}"))?;

        let queued = SyncOutboxItem {
            id,
            account_id: item.account_id,
            provider: item.provider,
            operation: item.operation,
            payload_json: item.payload_json,
            status: "pending".into(),
            attempt_count: 0,
            created_at,
            last_attempted_at: None,
            last_synced_at: None,
            last_error: None,
            next_attempt_at: None,
            failure_kind: None,
        };
        self.refresh_status_from_outbox();
        Ok(queued)
    }

    /// Selects `retryable` alongside `pending`, gated on `next_attempt_at`.
    /// Without the `retryable` arm a failed item would be unreachable forever.
    fn pending_outbox(&self) -> Vec<SyncOutboxItem> {
        let now = now_iso8601();
        self.connection
            .prepare(
                "SELECT id, account_id, provider, operation, payload_json, status, attempt_count,
                        created_at, last_attempted_at, last_synced_at, last_error,
                        next_attempt_at, failure_kind
                 FROM sync_outbox
                 WHERE status = 'pending'
                    OR (status = 'retryable'
                        AND (next_attempt_at IS NULL OR next_attempt_at <= ?1))
                 ORDER BY created_at, rowid",
            )
            .and_then(|mut statement| {
                statement
                    .query_map(params![now], outbox_from_row)
                    .and_then(|rows| rows.collect())
            })
            .unwrap_or_default()
    }

    fn outbox_entries(&self) -> Vec<SyncOutboxItem> {
        self.connection
            .prepare(
                "SELECT id, account_id, provider, operation, payload_json, status, attempt_count,
                        created_at, last_attempted_at, last_synced_at, last_error,
                        next_attempt_at, failure_kind
                 FROM sync_outbox ORDER BY created_at, rowid",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([], outbox_from_row)
                    .and_then(|rows| rows.collect())
            })
            .unwrap_or_default()
    }

    fn outbox_summary(&self) -> SyncOutboxSummary {
        let count = |clause: &str| -> usize {
            self.connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM sync_outbox WHERE {clause}"),
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0) as usize
        };

        SyncOutboxSummary {
            pending: count("status IN ('pending', 'retryable')"),
            synced: count("status = 'synced'"),
            failed: count("status = 'failed'"),
        }
    }

    fn mark_outbox_synced(&mut self, id: &str) -> Result<(), String> {
        let now = now_iso8601();
        let changed = self
            .connection
            .execute(
                "UPDATE sync_outbox SET status = 'synced', attempt_count = attempt_count + 1,
                     last_attempted_at = ?2, last_synced_at = ?2, last_error = NULL,
                     next_attempt_at = NULL, failure_kind = NULL
                 WHERE id = ?1",
                params![id, now],
            )
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err(format!("Unknown outbox item: {id}"));
        }
        self.refresh_status_from_outbox();
        Ok(())
    }

    /// A failure is `retryable` with a backoff until the attempt cap, then parks
    /// as `failed`. Writing `failed` on the first attempt would strand the item
    /// forever, because `pending_outbox` only selects pending and retryable.
    fn mark_outbox_failed(&mut self, id: &str, error: String) -> Result<(), String> {
        let attempts = self
            .connection
            .query_row(
                "SELECT attempt_count FROM sync_outbox WHERE id = ?1",
                params![id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("Unknown outbox item: {id}"))?
            + 1;

        let parked = attempts >= OUTBOX_ATTEMPT_CAP;
        let now = now_iso8601();

        self.connection
            .execute(
                "UPDATE sync_outbox SET status = ?2, attempt_count = ?3,
                     last_attempted_at = ?4, last_error = ?5,
                     next_attempt_at = ?6, failure_kind = ?7
                 WHERE id = ?1",
                params![
                    id,
                    if parked { "failed" } else { "retryable" },
                    attempts,
                    now,
                    error,
                    if parked {
                        None
                    } else {
                        Some(retry_at(attempts))
                    },
                    if parked { "permanent" } else { "transient" },
                ],
            )
            .map_err(|error| error.to_string())?;

        self.refresh_status_from_outbox();
        Ok(())
    }

    fn set_sync_status(&mut self, status: SyncStatus) {
        self.sync_status = status;
    }

    fn sync_status(&self) -> SyncStatus {
        self.sync_status.clone()
    }
}

fn prefixed_columns() -> String {
    SqliteMailStorage::MESSAGE_COLUMNS
        .split(", ")
        .map(|column| format!("m.{}", column.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn sync_state_from_row(row: &Row<'_>) -> rusqlite::Result<ProviderSyncState> {
    let slug: String = row.get(1)?;
    Ok(ProviderSyncState {
        account_id: row.get(0)?,
        provider: ProviderKind::from_slug(&slug).unwrap_or(ProviderKind::MockGmail),
        remote_account_id: row.get(2)?,
        sync_cursor: row.get(3)?,
        history_id: row.get(4)?,
        delta_token: row.get(5)?,
        page_token: row.get(6)?,
        high_watermark: row.get(7)?,
        status: row.get(8)?,
        last_attempted_at: row.get(9)?,
        last_successful_at: row.get(10)?,
        last_error: row.get(11)?,
    })
}

fn outbox_from_row(row: &Row<'_>) -> rusqlite::Result<SyncOutboxItem> {
    let slug: String = row.get(2)?;
    Ok(SyncOutboxItem {
        id: row.get(0)?,
        account_id: row.get(1)?,
        provider: ProviderKind::from_slug(&slug).unwrap_or(ProviderKind::MockGmail),
        operation: row.get(3)?,
        payload_json: row.get(4)?,
        status: row.get(5)?,
        attempt_count: row.get::<_, i64>(6)? as usize,
        created_at: row.get(7)?,
        last_attempted_at: row.get(8)?,
        last_synced_at: row.get(9)?,
        last_error: row.get(10)?,
        next_attempt_at: row.get(11)?,
        failure_kind: row.get(12)?,
    })
}

fn encode_string_list(values: &[String]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".into())
}

fn decode_string_list(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}
