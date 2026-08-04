-- Lotus durable mailbox schema.
--
-- Note on NOT NULL: sender_name and snippet default to the empty string rather
-- than forbidding null. Real mail has senders whose From header is a bare
-- address, and automated mail with an empty body yields no snippet. Forbidding
-- null there turns an ordinary message into a constraint violation mid-page,
-- which the "malformed MIME degrades one message" handling cannot catch because
-- it is a database error, not a parse error.

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS schema_version (
  version INTEGER NOT NULL,
  applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS connected_accounts (
  id TEXT PRIMARY KEY,
  -- The machine discriminant (ProviderKind::slug), not the display label.
  provider TEXT NOT NULL,
  provider_account_id TEXT NOT NULL,
  display_name TEXT NOT NULL,
  provider_label TEXT NOT NULL DEFAULT '',
  email_address TEXT NOT NULL,
  -- The keychain account key. Never a token value.
  credential_ref TEXT NOT NULL,
  accent TEXT NOT NULL,
  connected INTEGER NOT NULL DEFAULT 1 CHECK (connected IN (0, 1)),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (provider, provider_account_id),
  UNIQUE (provider, email_address)
);

CREATE TABLE IF NOT EXISTS provider_sync_state (
  account_id TEXT PRIMARY KEY REFERENCES connected_accounts(id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  remote_account_id TEXT NOT NULL,
  sync_cursor TEXT,
  history_id TEXT,
  delta_token TEXT,
  page_token TEXT,
  high_watermark TEXT,
  status TEXT NOT NULL DEFAULT 'idle',
  last_attempted_at TEXT,
  last_successful_at TEXT,
  last_error TEXT,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS mail_folders (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES connected_accounts(id) ON DELETE CASCADE,
  -- For Gmail this is the label id, e.g. INBOX. NULL for folders with no
  -- provider counterpart: Archive is the absence of INBOX, not a label. NULL
  -- rather than '' because SQLite treats NULLs as distinct in a UNIQUE index,
  -- so several local-only folders per account stay legal.
  provider_folder_id TEXT,
  name TEXT NOT NULL,
  role TEXT NOT NULL,
  unread_count INTEGER NOT NULL DEFAULT 0,
  sync_cursor TEXT,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (account_id, provider_folder_id)
);

CREATE TABLE IF NOT EXISTS mail_messages (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES connected_accounts(id) ON DELETE CASCADE,
  provider_message_id TEXT NOT NULL,
  provider_thread_id TEXT,
  sender_name TEXT NOT NULL DEFAULT '',
  sender_email TEXT NOT NULL DEFAULT '',
  subject TEXT NOT NULL DEFAULT '',
  snippet TEXT NOT NULL DEFAULT '',
  -- ISO-8601 UTC for display; internal_date is the sort key.
  received_at TEXT NOT NULL,
  internal_date INTEGER NOT NULL DEFAULT 0,
  unread INTEGER NOT NULL DEFAULT 0 CHECK (unread IN (0, 1)),
  starred INTEGER NOT NULL DEFAULT 0 CHECK (starred IN (0, 1)),
  provider_etag TEXT,
  provider_modseq TEXT,
  to_json TEXT NOT NULL DEFAULT '[]',
  cc_json TEXT NOT NULL DEFAULT '[]',
  reply_to_json TEXT NOT NULL DEFAULT '[]',
  body_json TEXT NOT NULL DEFAULT '[]',
  body_text TEXT NOT NULL DEFAULT '',
  body_html TEXT,
  deleted_at TEXT,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (account_id, provider_message_id)
);

CREATE TABLE IF NOT EXISTS mail_message_folders (
  message_id TEXT NOT NULL REFERENCES mail_messages(id) ON DELETE CASCADE,
  folder_id TEXT NOT NULL REFERENCES mail_folders(id) ON DELETE CASCADE,
  provider_uid TEXT,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (message_id, folder_id)
);

CREATE TABLE IF NOT EXISTS mail_message_labels (
  message_id TEXT NOT NULL REFERENCES mail_messages(id) ON DELETE CASCADE,
  label TEXT NOT NULL,
  PRIMARY KEY (message_id, label)
);

-- Gmail label id to display name, for labels that are not Lotus folders.
CREATE TABLE IF NOT EXISTS mail_labels (
  account_id TEXT NOT NULL REFERENCES connected_accounts(id) ON DELETE CASCADE,
  provider_label_id TEXT NOT NULL,
  name TEXT NOT NULL,
  colour TEXT,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (account_id, provider_label_id)
);

CREATE TABLE IF NOT EXISTS sync_outbox (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES connected_accounts(id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  operation TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  -- 'retryable' is what makes a failed item reachable again. Without it a
  -- failure is terminal, because the pending query would never select it.
  status TEXT NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending', 'retryable', 'synced', 'failed')),
  attempt_count INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  last_attempted_at TEXT,
  last_synced_at TEXT,
  last_error TEXT,
  next_attempt_at TEXT,
  failure_kind TEXT
);

-- Monotonic source for sync_outbox ids. MAX(rowid) is not safe: deleting an
-- account cascades its outbox rows away, freeing the highest rowid, and the next
-- insert would then reuse an id a surviving row already holds.
CREATE TABLE IF NOT EXISTS outbox_id_sequence (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  issued_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS provider_sync_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  account_id TEXT NOT NULL REFERENCES connected_accounts(id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  status TEXT NOT NULL,
  started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  finished_at TEXT,
  messages_seen INTEGER NOT NULL DEFAULT 0,
  messages_upserted INTEGER NOT NULL DEFAULT 0,
  folders_seen INTEGER NOT NULL DEFAULT 0,
  next_sync_cursor TEXT,
  next_history_id TEXT,
  next_delta_token TEXT,
  next_page_token TEXT,
  error TEXT
);

CREATE INDEX IF NOT EXISTS idx_mail_folders_account_role
  ON mail_folders(account_id, role);

-- Sorts on internal_date, not received_at: epoch millis order correctly
-- regardless of timestamp formatting.
CREATE INDEX IF NOT EXISTS idx_mail_messages_account_received
  ON mail_messages(account_id, internal_date DESC);

CREATE INDEX IF NOT EXISTS idx_mail_messages_sender_subject
  ON mail_messages(sender_email, subject);

CREATE INDEX IF NOT EXISTS idx_mail_message_folders_folder
  ON mail_message_folders(folder_id);

CREATE INDEX IF NOT EXISTS idx_sync_outbox_status_created
  ON sync_outbox(status, created_at);

CREATE INDEX IF NOT EXISTS idx_sync_outbox_account_status
  ON sync_outbox(account_id, status);

CREATE INDEX IF NOT EXISTS idx_sync_outbox_next_attempt
  ON sync_outbox(status, next_attempt_at);

CREATE INDEX IF NOT EXISTS idx_provider_sync_events_account_started
  ON provider_sync_events(account_id, started_at DESC);
