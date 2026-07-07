PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS connected_accounts (
  id TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  provider_account_id TEXT NOT NULL,
  display_name TEXT NOT NULL,
  email_address TEXT NOT NULL,
  credential_ref TEXT NOT NULL,
  accent TEXT NOT NULL,
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
  provider_folder_id TEXT NOT NULL,
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
  sender_name TEXT NOT NULL,
  sender_email TEXT NOT NULL,
  subject TEXT NOT NULL,
  snippet TEXT NOT NULL,
  received_at TEXT NOT NULL,
  unread INTEGER NOT NULL DEFAULT 0 CHECK (unread IN (0, 1)),
  starred INTEGER NOT NULL DEFAULT 0 CHECK (starred IN (0, 1)),
  provider_etag TEXT,
  provider_modseq TEXT,
  body_json TEXT NOT NULL DEFAULT '[]',
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

CREATE TABLE IF NOT EXISTS sync_outbox (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES connected_accounts(id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  operation TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending', 'synced', 'failed')),
  attempt_count INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  last_attempted_at TEXT,
  last_synced_at TEXT,
  last_error TEXT
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

CREATE INDEX IF NOT EXISTS idx_mail_messages_account_received
  ON mail_messages(account_id, received_at DESC);

CREATE INDEX IF NOT EXISTS idx_mail_messages_sender_subject
  ON mail_messages(sender_email, subject);

CREATE INDEX IF NOT EXISTS idx_mail_message_folders_folder
  ON mail_message_folders(folder_id);

CREATE INDEX IF NOT EXISTS idx_sync_outbox_status_created
  ON sync_outbox(status, created_at);

CREATE INDEX IF NOT EXISTS idx_sync_outbox_account_status
  ON sync_outbox(account_id, status);

CREATE INDEX IF NOT EXISTS idx_provider_sync_events_account_started
  ON provider_sync_events(account_id, started_at DESC);
