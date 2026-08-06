//! Tauri builder, command surface, and application state.
//!
//! Commands are `async fn` and Tauri drives them on its own runtime. Nothing
//! here constructs a runtime or calls `block_on`: a nested runtime panics at run
//! time with "cannot start a runtime from within a runtime", and it would
//! surface on the first real HTTP call rather than at compile time.

mod credentials;
mod model;
mod provider;
mod storage;
mod sync;

use std::collections::HashMap;
use std::sync::Arc;

use tauri::{Emitter, Manager};
use tokio::sync::Mutex;

use crate::credentials::CredentialStore;
use crate::model::*;
use crate::provider::gmail::auth::{ClientConfig, PendingLogin};
use crate::provider::gmail::{self, GmailProvider};
use crate::provider::ProviderRegistry;
use crate::storage::sqlite::SqliteMailStorage;
use crate::storage::MailStorage;
use crate::sync::LocalFirstSyncEngine;

/// The single event channel from Rust to Elm.
const EVENT_CHANNEL: &str = "lotus://event";

/// Payload for `lotus://event`. `login_state` is the correlation id: Elm
/// discards any event whose state does not match the flow it is showing, so a
/// second login attempt or an abandoned consent flow cannot hijack the UI.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LotusEvent {
    kind: String,
    login_state: Option<String>,
    message: Option<String>,
    progress: Option<SyncProgress>,
    bootstrap: Option<BootstrapData>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncProgress {
    imported: usize,
    total: Option<usize>,
    detail: String,
}

struct AppState {
    storage: Mutex<Box<dyn MailStorage>>,
    providers: ProviderRegistry,
    sync_engine: LocalFirstSyncEngine,
    credentials: Arc<CredentialStore>,
    /// At most one in-flight browser login per provider. Two concurrent consent
    /// flows would produce two callbacks that Elm cannot tell apart.
    active_logins: Mutex<HashMap<ProviderKind, String>>,
    gmail_setup_error: Option<String>,
}

impl AppState {
    async fn options(&self) -> Vec<ProviderOption> {
        self.providers.options()
    }
}

#[tauri::command]
async fn app_bootstrap(state: tauri::State<'_, AppState>) -> Result<BootstrapData, String> {
    let options = state.options().await;
    let storage = state.storage.lock().await;
    Ok(storage.bootstrap(options))
}

/// Returns as soon as the browser opens. Completion arrives later as a
/// `lotus://event`, because the callback lands on a loopback socket rather than
/// in a command response.
#[tauri::command]
async fn begin_account_login(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    provider: ProviderKind,
) -> Result<AccountLogin, String> {
    if !provider.uses_browser_login() {
        return state.providers.get(provider)?.begin_login().await;
    }

    if let Some(error) = &state.gmail_setup_error {
        return Err(error.clone());
    }

    {
        let active = state.active_logins.lock().await;
        if active.contains_key(&provider) {
            return Err(
                "A Google sign-in is already in progress. Finish it in your browser, or wait \
                 for it to time out."
                    .into(),
            );
        }
    }

    let config = ClientConfig::from_env()?;
    // Binds the loopback listener before the browser opens, so the macOS
    // firewall prompt lands while the user is still looking at Lotus.
    let pending = PendingLogin::begin(&config).await?;
    let login_state = pending.state.clone();
    let authorization_url = pending.authorization_url.clone();

    state
        .active_logins
        .lock()
        .await
        .insert(provider, login_state.clone());

    tauri_plugin_opener::open_url(&authorization_url, None::<&str>)
        .map_err(|error| format!("Could not open your browser for Google sign-in: {error}"))?;

    let handle = app.clone();
    let correlation = login_state.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = finish_gmail_login(&handle, pending).await;
        let state = handle.state::<AppState>();
        state
            .active_logins
            .lock()
            .await
            .remove(&ProviderKind::Gmail);

        let event = match outcome {
            Ok(bootstrap) => LotusEvent {
                kind: "login.completed".into(),
                login_state: Some(correlation),
                message: None,
                progress: None,
                bootstrap: Some(bootstrap),
            },
            Err(error) => LotusEvent {
                kind: "login.failed".into(),
                login_state: Some(correlation),
                message: Some(error),
                progress: None,
                bootstrap: None,
            },
        };
        let _ = handle.emit(EVENT_CHANNEL, event);
    });

    Ok(AccountLogin {
        provider,
        login_url: authorization_url,
        login_state,
        expires_at: expiry_five_minutes(),
        scopes: vec![provider::gmail::auth::GMAIL_SCOPE.to_string()],
        browser_login: true,
    })
}

/// Await the callback, import the first window, then continue paginating in the
/// background. A failed page leaves everything already imported readable.
async fn finish_gmail_login(
    app: &tauri::AppHandle,
    pending: PendingLogin,
) -> Result<BootstrapData, String> {
    let (code, verifier, redirect_uri) = pending.wait_for_code().await?;

    let state = app.state::<AppState>();
    let provider_ref = state.providers.get(ProviderKind::Gmail)?;
    let gmail = provider_ref
        .as_any()
        .downcast_ref::<GmailProvider>()
        .ok_or_else(|| "Gmail provider is not registered.".to_string())?;

    let connection = gmail
        .complete_oauth(&code, &verifier, &redirect_uri)
        .await?;

    {
        let storage = state.storage.lock().await;
        if storage
            .account_by_provider_id(ProviderKind::Gmail, &connection.email_address)
            .is_some()
        {
            return Err(format!(
                "{} is already connected to Lotus.",
                connection.email_address
            ));
        }
    }

    let all_labels = gmail
        .labels(&connection.access_token)
        .await
        .map_err(|error| error.message)?;
    let folders = gmail::labels::folders_for_account(&connection.account_id, &all_labels);

    // Capture the history id from the profile call *before* listing messages.
    // Persisting the post-import value silently drops every change that landed
    // during a long first import.
    let pre_sync_history_id = connection.history_id.clone();

    let (refs, next_page_token) = gmail
        .list_inbox_page(&connection.access_token, None, gmail::FIRST_WINDOW.min(500))
        .await
        .map_err(|error| error.message)?;

    let messages = gmail
        .fetch_messages(
            &connection.access_token,
            &connection.account_id,
            &folders,
            &all_labels,
            &refs,
        )
        .await;

    let imported = messages.len();
    let seed = AccountSeed {
        account: Account {
            id: connection.account_id.clone(),
            display_name: "Gmail".into(),
            email_address: connection.email_address.clone(),
            provider: "Gmail".into(),
            provider_kind: ProviderKind::Gmail,
            accent: "#c24135".into(),
            connected: true,
        },
        folders: folders.clone(),
        messages,
        credential: StoredCredential {
            account_id: connection.account_id.clone(),
            provider: ProviderKind::Gmail,
            access_token: connection.access_token.clone(),
            refresh_token: String::new(),
            expires_at: connection.expires_at.clone(),
        },
        sync_state: ProviderSyncState {
            account_id: connection.account_id.clone(),
            provider: ProviderKind::Gmail,
            remote_account_id: connection.email_address.clone(),
            sync_cursor: None,
            history_id: pre_sync_history_id,
            delta_token: None,
            page_token: next_page_token.clone(),
            high_watermark: Some(now_iso8601()),
            status: "idle".into(),
            last_attempted_at: Some(now_iso8601()),
            last_successful_at: Some(now_iso8601()),
            last_error: None,
        },
    };

    let options = state.options().await;
    let bootstrap = {
        let mut storage = state.storage.lock().await;
        storage.add_account(seed, options)?
    };

    let _ = app.emit(
        EVENT_CHANNEL,
        LotusEvent {
            kind: "sync.progress".into(),
            login_state: None,
            message: None,
            progress: Some(SyncProgress {
                imported,
                total: None,
                detail: format!("Imported {imported} messages"),
            }),
            bootstrap: None,
        },
    );

    if let Some(token) = next_page_token {
        spawn_background_import(app.clone(), connection.account_id.clone(), token);
    }

    Ok(bootstrap)
}

/// Continue pagination after the first window, stopping at `MESSAGE_CAP`.
fn spawn_background_import(app: tauri::AppHandle, account_id: String, first_token: String) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let mut page_token = Some(first_token);

        while let Some(token) = page_token.clone() {
            // One lock for both reads. Taking it twice in a row is a wasted round
            // trip and gives another writer a window in between.
            let (sync_state, imported_so_far) = {
                let storage = state.storage.lock().await;
                match storage.sync_state(&account_id) {
                    Some(value) => (value, storage.message_count(&account_id)),
                    // The account was disconnected mid-import.
                    None => return,
                }
            };

            if imported_so_far >= gmail::MESSAGE_CAP {
                let _ = app.emit(
                    EVENT_CHANNEL,
                    LotusEvent {
                        kind: "sync.progress".into(),
                        login_state: None,
                        message: None,
                        progress: Some(SyncProgress {
                            imported: imported_so_far,
                            total: Some(gmail::MESSAGE_CAP),
                            detail: format!(
                                "Showing your {} most recent messages",
                                gmail::MESSAGE_CAP
                            ),
                        }),
                        bootstrap: None,
                    },
                );
                return;
            }

            let Ok(provider_ref) = state.providers.get(ProviderKind::Gmail) else {
                return;
            };
            let Some(gmail_provider) = provider_ref.as_any().downcast_ref::<GmailProvider>() else {
                return;
            };

            let reference = credentials::credential_ref(
                ProviderKind::Gmail.slug(),
                &sync_state.remote_account_id,
            );
            let Ok(access_token) = gmail_provider.access_token(&account_id, &reference).await
            else {
                return;
            };

            let all_labels = match gmail_provider.labels(&access_token).await {
                Ok(labels) => labels,
                Err(_) => return,
            };
            let folders = gmail::labels::folders_for_account(&account_id, &all_labels);

            let remaining = gmail::MESSAGE_CAP.saturating_sub(imported_so_far);
            let (refs, next) = match gmail_provider
                .list_inbox_page(&access_token, Some(&token), remaining.min(200))
                .await
            {
                Ok(page) => page,
                Err(_) => return,
            };

            if refs.is_empty() {
                return;
            }

            let messages = gmail_provider
                .fetch_messages(&access_token, &account_id, &folders, &all_labels, &refs)
                .await;
            let batch = messages.len();

            let mut next_state = sync_state.clone();
            next_state.page_token = next.clone();
            next_state.last_successful_at = Some(now_iso8601());

            {
                let mut storage = state.storage.lock().await;
                if let Err(error) = storage.apply_mailbox_delta(MailboxDelta {
                    // Pass the folders through. A label created between login and
                    // this page produces a membership row for a folder that was
                    // never written, and the whole transaction dies on a foreign
                    // key violation.
                    folders: folders.clone(),
                    messages,
                    sync_state: next_state,
                }) {
                    // The page token lived only inside that failed transaction,
                    // so record it on its own before giving up. Otherwise the
                    // import stops here permanently: nothing else resumes it.
                    let mut checkpoint = sync_state.clone();
                    checkpoint.page_token = next.clone();
                    checkpoint.status = "failed".into();
                    checkpoint.last_attempted_at = Some(now_iso8601());
                    checkpoint.last_error = Some(error);
                    let _ = storage.save_sync_state(checkpoint);
                    return;
                }
            }

            let total = {
                let storage = state.storage.lock().await;
                storage.message_count(&account_id)
            };
            let _ = app.emit(
                EVENT_CHANNEL,
                LotusEvent {
                    kind: "sync.progress".into(),
                    login_state: None,
                    message: None,
                    progress: Some(SyncProgress {
                        imported: total,
                        total: None,
                        detail: format!("Imported {total} messages ({batch} in the last page)"),
                    }),
                    bootstrap: None,
                },
            );

            page_token = next;
        }
    });
}

/// Mock providers only. Gmail completes through the loopback callback.
#[tauri::command]
async fn complete_account_login(
    state: tauri::State<'_, AppState>,
    provider: ProviderKind,
    login_state: String,
    email_address: String,
) -> Result<AccountSetupResult, String> {
    let seed = state
        .providers
        .get(provider)?
        .complete_login(&login_state, &email_address)
        .await?;
    let credential = seed.credential.preview();

    let options = state.options().await;
    let mut storage = state.storage.lock().await;
    let bootstrap = storage.add_account(seed, options)?;
    Ok(AccountSetupResult {
        bootstrap,
        credential,
    })
}

#[tauri::command]
async fn disconnect_account(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<BootstrapData, String> {
    let reference = {
        let storage = state.storage.lock().await;
        storage.credential_ref(&account_id)
    };

    if let Some(reference) = reference {
        // Best effort: a missing keychain entry must not block the disconnect.
        let _ = state.credentials.delete_refresh_token(&reference);
    }
    state.credentials.clear_access_token(&account_id).await;

    let options = state.options().await;
    let mut storage = state.storage.lock().await;
    storage.remove_account(&account_id)?;
    Ok(storage.refresh(options))
}

#[tauri::command]
async fn provider_sync_checkpoint(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<ProviderSyncCheckpoint, String> {
    let storage = state.storage.lock().await;
    storage
        .sync_state(&account_id)
        .map(ProviderSyncCheckpoint::from)
        .ok_or_else(|| format!("No provider sync checkpoint for account: {account_id}"))
}

#[tauri::command]
async fn sync_pending_changes(state: tauri::State<'_, AppState>) -> Result<SyncReport, String> {
    state
        .sync_engine
        .sync_once(&state.storage, &state.providers)
        .await
}

#[tauri::command]
async fn sync_outbox_status(
    state: tauri::State<'_, AppState>,
) -> Result<SyncOutboxSummary, String> {
    let storage = state.storage.lock().await;
    Ok(storage.outbox_summary())
}

#[tauri::command]
async fn sync_outbox_entries(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<SyncOutboxEntry>, String> {
    let storage = state.storage.lock().await;
    Ok(storage
        .outbox_entries()
        .into_iter()
        .map(SyncOutboxEntry::from)
        .collect())
}

#[tauri::command]
async fn select_folder(
    state: tauri::State<'_, AppState>,
    folder_id: String,
) -> Result<MailboxSnapshot, String> {
    let mut storage = state.storage.lock().await;
    storage.select_folder(&folder_id)
}

#[tauri::command]
async fn select_message(
    state: tauri::State<'_, AppState>,
    message_id: String,
) -> Result<MessageUpdate, String> {
    let mut storage = state.storage.lock().await;
    storage.select_message(&message_id)
}

#[tauri::command]
async fn search_messages(
    state: tauri::State<'_, AppState>,
    query: String,
) -> Result<MailboxSnapshot, String> {
    let mut storage = state.storage.lock().await;
    Ok(storage.search(&query))
}

#[tauri::command]
async fn mark_message_read(
    state: tauri::State<'_, AppState>,
    message_id: String,
    read: bool,
) -> Result<MessageUpdate, String> {
    let mut storage = state.storage.lock().await;
    storage.mark_message_read(&message_id, read)
}

#[tauri::command]
async fn archive_message(
    state: tauri::State<'_, AppState>,
    message_id: String,
) -> Result<MailboxSnapshot, String> {
    let mut storage = state.storage.lock().await;
    storage.archive_message(&message_id)
}

#[tauri::command]
async fn refresh_mail(state: tauri::State<'_, AppState>) -> Result<BootstrapData, String> {
    state
        .sync_engine
        .sync_once(&state.storage, &state.providers)
        .await?;
    let options = state.options().await;
    let mut storage = state.storage.lock().await;
    Ok(storage.refresh(options))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("Could not resolve the app data directory: {error}"))?;
            std::fs::create_dir_all(&data_dir)
                .map_err(|error| format!("Could not create {}: {error}", data_dir.display()))?;

            let storage = SqliteMailStorage::open(&data_dir.join("mail.sqlite3"))?;

            let credentials = Arc::new(CredentialStore::new());
            let mut providers = ProviderRegistry::with_mocks();

            // Gmail registers only when it is configured. A missing config is a
            // setup problem to surface on click, not a reason to fail startup.
            let gmail_setup_error = match ClientConfig::from_env() {
                Ok(config) => {
                    providers.insert(Box::new(GmailProvider::new(config, credentials.clone())));
                    None
                }
                Err(error) => Some(error),
            };

            app.manage(AppState {
                storage: Mutex::new(Box::new(storage)),
                providers,
                sync_engine: LocalFirstSyncEngine,
                credentials,
                active_logins: Mutex::new(HashMap::new()),
                gmail_setup_error,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_bootstrap,
            begin_account_login,
            complete_account_login,
            disconnect_account,
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

/// Test-only surface. The loopback OAuth listener is the highest-risk mechanism
/// in the app: a socket, a hand-parsed HTTP request, and a constant-time
/// comparison. `tests/oauth_loopback.rs` drives it over a real TCP connection,
/// which needs these two items to be reachable from an integration test.
pub mod testing {
    pub use crate::provider::gmail::auth::{ClientConfig, PendingLogin};

    pub async fn begin_login(config: &ClientConfig) -> Result<PendingLogin, String> {
        PendingLogin::begin(config).await
    }
}

/// Serializes a deterministic `app_bootstrap` payload for the wire-shape golden
/// test. Public so `src/bin/wire-fixture.rs` can call it; built by hand rather
/// than from a live provider so it needs no database and never drifts with time.
pub fn wire_fixture_json() -> String {
    let detail = MessageDetail {
        id: "gmail-reader-example-com-18c2f".into(),
        account_id: "gmail-reader-example-com".into(),
        folder_ids: vec![
            "gmail-reader-example-com-inbox".into(),
            "gmail-reader-example-com-label-label-3".into(),
        ],
        sender_name: "Ada Lovelace".into(),
        sender_email: "ada@example.com".into(),
        subject: "Analytical Engine notes".into(),
        snippet: "The first paragraph of the message body.".into(),
        received_at: "2026-07-01T12:10:00Z".into(),
        internal_date: 1_751_372_600_000,
        unread: true,
        starred: false,
        labels: vec!["Receipts".into()],
        to: vec!["reader@example.com".into()],
        cc: vec!["cc@example.com".into()],
        reply_to: vec![],
        body_paragraphs: vec![
            "The first paragraph of the message body.".into(),
            "The second paragraph.".into(),
        ],
        provider_message_id: Some("18c2f".into()),
        provider_thread_id: Some("18c2e".into()),
    };

    let bootstrap = BootstrapData {
        provider_options: vec![ProviderOption {
            provider: ProviderKind::Gmail,
            display_name: "Gmail".into(),
            description: "Sign in with Google to sync your real inbox".into(),
            browser_login: true,
        }],
        accounts: vec![Account {
            id: "gmail-reader-example-com".into(),
            display_name: "Gmail".into(),
            email_address: "reader@example.com".into(),
            provider: "Gmail".into(),
            provider_kind: ProviderKind::Gmail,
            accent: "#c24135".into(),
            connected: true,
        }],
        folders: vec![Folder {
            id: "gmail-reader-example-com-inbox".into(),
            account_id: "gmail-reader-example-com".into(),
            name: "Inbox".into(),
            role: "inbox".into(),
            provider_folder_id: Some("INBOX".into()),
            unread_count: 1,
        }],
        messages: vec![detail.summary()],
        selected_folder_id: UNIFIED_INBOX_ID.into(),
        selected_message_id: Some(detail.id.clone()),
        selected_message: Some(detail),
        sync_status: SyncStatus {
            state: "Synced".into(),
            last_checked: Some("2026-07-01T12:11:00Z".into()),
            detail: "1 messages cached locally".into(),
        },
    };

    serde_json::to_string_pretty(&bootstrap).unwrap_or_default()
}

fn expiry_five_minutes() -> String {
    let deadline = time::OffsetDateTime::now_utc() + time::Duration::minutes(5);
    deadline
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| now_iso8601())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::gmail::labels;
    use base64::Engine as _;

    fn base64_encode(value: &str) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value)
    }
    use crate::storage::memory::MailStore;

    fn mock_registry() -> ProviderRegistry {
        ProviderRegistry::with_mocks()
    }

    fn storage_variants() -> Vec<(&'static str, Box<dyn MailStorage>)> {
        vec![
            ("memory", Box::new(MailStore::empty())),
            (
                "sqlite",
                Box::new(SqliteMailStorage::in_memory().expect("in-memory database")),
            ),
        ]
    }

    #[tokio::test]
    async fn mock_login_imports_mailbox_across_both_storage_backends() {
        for (label, mut storage) in storage_variants() {
            let providers = mock_registry();
            let provider = providers.get(ProviderKind::MockGmail).unwrap();
            let login = provider.begin_login().await.unwrap();
            let seed = provider
                .complete_login(&login.login_state, "reader@gmail.test")
                .await
                .unwrap();
            let credential = seed.credential.preview();

            let bootstrap = storage.add_account(seed, providers.options()).unwrap();

            assert_eq!(bootstrap.accounts.len(), 1, "{label}: one account");
            assert_eq!(bootstrap.folders.len(), 5, "{label}: five folders");
            // Two inbox messages; the receipt seeds Archive.
            assert_eq!(bootstrap.messages.len(), 2, "{label}: inbox messages");
            assert_eq!(storage.outbox_summary().pending, 1, "{label}: outbox");
            assert_eq!(credential.refresh_token_tail, "-state");
            assert_eq!(bootstrap.selected_folder_id, UNIFIED_INBOX_ID);
            assert!(bootstrap.selected_message_id.is_some());

            // Credentials are asserted through the trait, never by reaching into
            // private state.
            assert!(storage
                .credential_ref("mock-gmail-reader-gmail-test")
                .is_some());

            let mut sync_state = storage.sync_state("mock-gmail-reader-gmail-test").unwrap();
            assert_eq!(sync_state.provider, ProviderKind::MockGmail);
            assert_eq!(sync_state.remote_account_id, "reader@gmail.test");
            assert_eq!(
                sync_state.sync_cursor.as_deref(),
                Some("mock-gmail-cursor-1")
            );
            assert_eq!(sync_state.status, "idle");
            assert_eq!(sync_state.last_error, None);

            sync_state.sync_cursor = Some("mock-gmail-cursor-2".into());
            storage.save_sync_state(sync_state).unwrap();
            let advanced = storage.sync_state("mock-gmail-reader-gmail-test").unwrap();
            assert_eq!(advanced.sync_cursor.as_deref(), Some("mock-gmail-cursor-2"));
        }
    }

    #[tokio::test]
    async fn sync_engine_drains_the_outbox_then_applies_the_delta() {
        for (label, storage) in storage_variants() {
            let providers = mock_registry();
            let provider = providers.get(ProviderKind::MockGmail).unwrap();
            let login = provider.begin_login().await.unwrap();
            let seed = provider
                .complete_login(&login.login_state, "reader@gmail.test")
                .await
                .unwrap();

            let storage = Mutex::new(storage);
            storage
                .lock()
                .await
                .add_account(seed, providers.options())
                .unwrap();

            let engine = LocalFirstSyncEngine;
            let report = engine.sync_once(&storage, &providers).await.unwrap();
            assert_eq!(report.attempted, 1, "{label}: account.connected drained");
            assert_eq!(report.inbound_messages, 1, "{label}: one inbound message");
            assert_eq!(report.remaining_pending, 0);

            {
                let guard = storage.lock().await;
                let inbox = guard.bootstrap(providers.options()).messages;
                assert_eq!(inbox.len(), 3, "{label}: inbox after sync");
                assert!(inbox
                    .iter()
                    .any(|m| m.id == "mock-gmail-reader-gmail-test-provider-sync-2"));
            }

            {
                let mut guard = storage.lock().await;
                guard
                    .select_message("mock-gmail-reader-gmail-test-welcome")
                    .unwrap();
                guard
                    .archive_message("mock-gmail-reader-gmail-test-welcome")
                    .unwrap();
                assert_eq!(guard.outbox_summary().pending, 2);
            }

            let second = engine.sync_once(&storage, &providers).await.unwrap();
            assert_eq!(second.synced, 2);
            assert_eq!(second.failed, 0);
            assert_eq!(second.remaining_pending, 0);
        }
    }

    #[tokio::test]
    async fn duplicate_account_is_rejected() {
        for (_, mut storage) in storage_variants() {
            let providers = mock_registry();
            let provider = providers.get(ProviderKind::MockOutlook).unwrap();
            let login = provider.begin_login().await.unwrap();

            let first = provider
                .complete_login(&login.login_state, "reader@outlook.test")
                .await
                .unwrap();
            storage.add_account(first, providers.options()).unwrap();

            let second = provider
                .complete_login(&login.login_state, "reader@outlook.test")
                .await
                .unwrap();
            let error = match storage.add_account(second, providers.options()) {
                Ok(_) => panic!("duplicate account should be rejected"),
                Err(error) => error,
            };
            assert!(error.contains("already connected"));
        }
    }

    /// Archiving removes inbox membership only. The message stays in the
    /// account's other folders.
    #[tokio::test]
    async fn a_message_can_live_in_two_folders_and_archive_removes_only_the_inbox() {
        for (label, mut storage) in storage_variants() {
            let providers = mock_registry();
            let provider = providers.get(ProviderKind::MockGmail).unwrap();
            let login = provider.begin_login().await.unwrap();
            let seed = provider
                .complete_login(&login.login_state, "reader@gmail.test")
                .await
                .unwrap();
            storage.add_account(seed, providers.options()).unwrap();

            let welcome = "mock-gmail-reader-gmail-test-welcome";
            let before = storage.select_message(welcome).unwrap().message;
            assert!(
                before.folder_ids.len() >= 2,
                "{label}: seeded into inbox and starred"
            );
            assert!(before
                .folder_ids
                .contains(&"mock-gmail-reader-gmail-test-inbox".to_string()));

            storage.archive_message(welcome).unwrap();

            let after = storage.select_message(welcome).unwrap().message;
            assert!(
                !after
                    .folder_ids
                    .contains(&"mock-gmail-reader-gmail-test-inbox".to_string()),
                "{label}: inbox membership removed"
            );
            assert!(
                after
                    .folder_ids
                    .contains(&"mock-gmail-reader-gmail-test-starred".to_string()),
                "{label}: other memberships survive"
            );
            assert!(after
                .folder_ids
                .contains(&"mock-gmail-reader-gmail-test-archive".to_string()));
        }
    }

    /// A message carrying two inbox-role labels must appear once, not twice.
    #[tokio::test]
    async fn the_unified_inbox_dedupes_multi_folder_messages() {
        for (label, mut storage) in storage_variants() {
            let providers = mock_registry();
            let provider = providers.get(ProviderKind::MockGmail).unwrap();
            let login = provider.begin_login().await.unwrap();
            let mut seed = provider
                .complete_login(&login.login_state, "reader@gmail.test")
                .await
                .unwrap();

            // Two inbox-role folders, and one message in both.
            let second_inbox = folder(
                "mock-gmail-reader-gmail-test-inbox-2",
                "mock-gmail-reader-gmail-test",
                "Priority",
                "inbox",
            );
            seed.folders.push(second_inbox);
            seed.messages[0]
                .folder_ids
                .push("mock-gmail-reader-gmail-test-inbox-2".into());

            let bootstrap = storage.add_account(seed, providers.options()).unwrap();
            let welcome_rows = bootstrap
                .messages
                .iter()
                .filter(|m| m.id == "mock-gmail-reader-gmail-test-welcome")
                .count();
            assert_eq!(welcome_rows, 1, "{label}: exactly one row");
        }
    }

    #[test]
    fn sqlite_persists_accounts_and_messages_across_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mail.sqlite3");
        let providers = mock_registry();

        {
            let mut storage = SqliteMailStorage::open(&path).unwrap();
            let provider = providers.get(ProviderKind::MockGmail).unwrap();
            let seed = tauri::async_runtime::block_on(
                provider.complete_login("state", "reader@gmail.test"),
            )
            .unwrap();
            storage.add_account(seed, providers.options()).unwrap();
        }

        let reopened = SqliteMailStorage::open(&path).unwrap();
        let bootstrap = reopened.bootstrap(providers.options());
        assert_eq!(bootstrap.accounts.len(), 1);
        assert_eq!(bootstrap.folders.len(), 5);
        assert_eq!(bootstrap.messages.len(), 2);
        assert_eq!(bootstrap.selected_folder_id, UNIFIED_INBOX_ID);
        assert!(bootstrap.selected_message_id.is_some());
    }

    /// Tokens must never reach SQLite. Grep the file for the token-shaped values
    /// the mock provider generates.
    #[test]
    fn no_token_value_appears_in_the_database_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mail.sqlite3");
        let providers = mock_registry();

        {
            let mut storage = SqliteMailStorage::open(&path).unwrap();
            let provider = providers.get(ProviderKind::MockGmail).unwrap();
            let seed = tauri::async_runtime::block_on(
                provider.complete_login("secretstate", "reader@gmail.test"),
            )
            .unwrap();
            assert!(seed.credential.access_token.contains("mock-access"));
            storage.add_account(seed, providers.options()).unwrap();
        }

        let bytes = std::fs::read(&path).unwrap();
        let haystack = String::from_utf8_lossy(&bytes);
        assert!(
            !haystack.contains("mock-access"),
            "an access token reached SQLite"
        );
        assert!(
            !haystack.contains("mock-refresh"),
            "a refresh token reached SQLite"
        );
        // The keychain reference is expected; it is not a token.
        assert!(haystack.contains("mock-gmail:reader@gmail.test"));
    }

    /// Two connected accounts must not see each other's mail. The starred view
    /// reads `starred = 1` rather than a folder membership, so it needs an
    /// explicit account filter that the folder path gets for free.
    #[test]
    fn one_accounts_starred_folder_never_shows_another_accounts_mail() {
        let mut storage = SqliteMailStorage::in_memory().unwrap();
        let providers = mock_registry();

        for (kind, address) in [
            (ProviderKind::MockGmail, "a@gmail.test"),
            (ProviderKind::MockOutlook, "b@outlook.test"),
        ] {
            let provider = providers.get(kind).unwrap();
            let seed =
                tauri::async_runtime::block_on(provider.complete_login("s", address)).unwrap();
            storage.add_account(seed, providers.options()).unwrap();
        }

        // Each mock seeds exactly one starred message, into its own account.
        for (account_id, folder_id) in [
            ("mock-gmail-a-gmail-test", "mock-gmail-a-gmail-test-starred"),
            (
                "mock-outlook-b-outlook-test",
                "mock-outlook-b-outlook-test-starred",
            ),
        ] {
            let rows = storage.select_folder(folder_id).unwrap().messages;
            assert_eq!(rows.len(), 1, "{folder_id} should hold one message");
            assert_eq!(
                rows[0].account_id, account_id,
                "{folder_id} leaked another account's mail"
            );
        }
    }

    /// A failed outbox item must stay reachable. `mark_outbox_failed` writing a
    /// terminal status while `pending_outbox` selects only pending and retryable
    /// would strand every local change forever.
    #[test]
    fn a_failed_outbox_item_becomes_retryable_and_drains_later() {
        for (label, mut storage) in storage_variants() {
            let providers = mock_registry();
            let provider = providers.get(ProviderKind::MockGmail).unwrap();
            let seed =
                tauri::async_runtime::block_on(provider.complete_login("s", "reader@gmail.test"))
                    .unwrap();
            storage.add_account(seed, providers.options()).unwrap();

            let queued = storage.pending_outbox();
            assert_eq!(queued.len(), 1, "{label}: the connect item is queued");

            let item_id = queued[0].id.clone();
            storage
                .mark_outbox_failed(&item_id, "network unreachable".into())
                .unwrap();

            // Retryable, not terminal, and still counted as outstanding work.
            let after = storage.outbox_entries();
            assert_eq!(
                after[0].status, "retryable",
                "{label}: a failure must stay reachable, not be stranded"
            );
            assert_eq!(after[0].attempt_count, 1);
            assert_eq!(after[0].failure_kind.as_deref(), Some("transient"));
            assert_eq!(storage.outbox_summary().pending, 1);
            assert_eq!(storage.outbox_summary().failed, 0);

            // The backoff gates the next attempt, so it is not selected yet.
            assert!(
                storage.pending_outbox().is_empty(),
                "{label}: the backoff should hold the item back briefly"
            );
            assert!(after[0].next_attempt_at.is_some());

            // Past the cap it parks: terminal status, no next attempt.
            for _ in 0..OUTBOX_ATTEMPT_CAP {
                storage
                    .mark_outbox_failed(&item_id, "still down".into())
                    .unwrap();
            }
            let parked = storage.outbox_entries();
            assert_eq!(parked[0].status, "failed", "{label}: past the cap it parks");
            assert_eq!(parked[0].next_attempt_at, None);
            assert_eq!(parked[0].failure_kind.as_deref(), Some("permanent"));
            assert_eq!(storage.outbox_summary().failed, 1);
            assert!(storage.pending_outbox().is_empty());
        }
    }

    /// Outbox ids must not be reused. Deriving one from MAX(rowid) breaks after a
    /// cascade delete frees the highest rowid, and then `mark_outbox_synced`
    /// addresses whichever row happens to hold that id.
    #[test]
    fn outbox_ids_are_never_reused_after_an_account_is_removed() {
        let mut storage = SqliteMailStorage::in_memory().unwrap();
        let providers = mock_registry();
        let mut seen: Vec<String> = Vec::new();

        for (kind, address) in [
            (ProviderKind::MockGmail, "a@gmail.test"),
            (ProviderKind::MockOutlook, "b@outlook.test"),
        ] {
            let provider = providers.get(kind).unwrap();
            let seed =
                tauri::async_runtime::block_on(provider.complete_login("s", address)).unwrap();
            storage.add_account(seed, providers.options()).unwrap();
        }
        seen.extend(storage.outbox_entries().into_iter().map(|item| item.id));

        storage
            .remove_account("mock-outlook-b-outlook-test")
            .unwrap();

        let provider = providers.get(ProviderKind::MockOutlook).unwrap();
        let seed =
            tauri::async_runtime::block_on(provider.complete_login("s", "c@outlook.test")).unwrap();
        storage.add_account(seed, providers.options()).unwrap();

        for item in storage.outbox_entries() {
            if !seen.contains(&item.id) {
                seen.push(item.id);
            } else {
                // Only legal if it is the same surviving row.
                assert_eq!(item.account_id, "mock-gmail-a-gmail-test");
            }
        }

        let ids: Vec<String> = storage.outbox_entries().into_iter().map(|i| i.id).collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(ids.len(), unique.len(), "outbox ids collided: {ids:?}");
    }

    /// A delta whose messages reference a folder the delta does not also create
    /// must not fail. Gmail labels can appear between login and a later import
    /// page, and dropping the folder list would break the whole transaction on a
    /// foreign key violation.
    #[test]
    fn a_delta_carrying_a_new_folder_and_its_messages_applies_cleanly() {
        let mut storage = SqliteMailStorage::in_memory().unwrap();
        let providers = mock_registry();
        let provider = providers.get(ProviderKind::MockGmail).unwrap();
        let seed =
            tauri::async_runtime::block_on(provider.complete_login("s", "reader@gmail.test"))
                .unwrap();
        let account_id = seed.account.id.clone();
        storage.add_account(seed, providers.options()).unwrap();

        let new_folder = Folder {
            id: format!("{account_id}-label-new"),
            account_id: account_id.clone(),
            name: "Newsletters".into(),
            role: "label".into(),
            provider_folder_id: Some("Label_99".into()),
            unread_count: 0,
        };
        let message = MessageDetail {
            id: format!("{account_id}-later"),
            account_id: account_id.clone(),
            folder_ids: vec![new_folder.id.clone()],
            sender_name: "List".into(),
            sender_email: "list@example.com".into(),
            subject: "Weekly".into(),
            snippet: "Body".into(),
            received_at: "2026-07-02T09:00:00Z".into(),
            internal_date: iso8601_to_epoch_millis("2026-07-02T09:00:00Z"),
            unread: true,
            starred: false,
            labels: vec!["Newsletters".into()],
            to: vec![],
            cc: vec![],
            reply_to: vec![],
            body_paragraphs: vec!["Body".into()],
            provider_message_id: Some("later".into()),
            provider_thread_id: None,
        };

        storage
            .apply_mailbox_delta(MailboxDelta {
                folders: vec![new_folder.clone()],
                messages: vec![message],
                sync_state: storage.sync_state(&account_id).unwrap(),
            })
            .expect("a delta that creates a folder and files mail into it must apply");

        let filed = storage.select_folder(&new_folder.id).unwrap().messages;
        assert_eq!(filed.len(), 1);
        assert_eq!(filed[0].subject, "Weekly");
    }

    #[test]
    fn migrations_are_idempotent_and_record_a_version() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mail.sqlite3");
        SqliteMailStorage::open(&path).unwrap();
        SqliteMailStorage::open(&path).unwrap();

        let connection = rusqlite::Connection::open(&path).unwrap();
        let rows: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1, "migration ran exactly once");
    }

    /// Real mail has unnamed senders and empty bodies. Neither may fail the
    /// upsert: a constraint violation here is a database error, which the
    /// degrade-one-message handling cannot catch.
    #[test]
    fn a_message_with_no_sender_name_and_no_body_upserts_cleanly() {
        let mut storage = SqliteMailStorage::in_memory().unwrap();
        let providers = mock_registry();
        let provider = providers.get(ProviderKind::MockGmail).unwrap();
        let seed =
            tauri::async_runtime::block_on(provider.complete_login("s", "reader@gmail.test"))
                .unwrap();
        let account_id = seed.account.id.clone();
        let inbox_id = seed.folders[0].id.clone();
        storage.add_account(seed, providers.options()).unwrap();

        let sync_state = storage.sync_state(&account_id).unwrap();
        let bare = MessageDetail {
            id: format!("{account_id}-bare"),
            account_id: account_id.clone(),
            folder_ids: vec![inbox_id],
            sender_name: String::new(),
            sender_email: "noreply@example.com".into(),
            subject: "Automated notice".into(),
            snippet: String::new(),
            received_at: "2026-07-01T09:00:00Z".into(),
            internal_date: iso8601_to_epoch_millis("2026-07-01T09:00:00Z"),
            unread: true,
            starred: false,
            labels: Vec::new(),
            to: Vec::new(),
            cc: Vec::new(),
            reply_to: Vec::new(),
            body_paragraphs: Vec::new(),
            provider_message_id: Some("bare".into()),
            provider_thread_id: None,
        };

        storage
            .apply_mailbox_delta(MailboxDelta {
                folders: Vec::new(),
                messages: vec![bare],
                sync_state,
            })
            .expect("empty sender name and snippet must not violate a constraint");

        let stored = storage
            .select_message(&format!("{account_id}-bare"))
            .unwrap()
            .message;
        assert_eq!(stored.subject, "Automated notice");
        assert_eq!(stored.snippet, "");
    }

    /// Messages sort by `internal_date`, so a Gmail import lands newest-first
    /// regardless of how the timestamps were formatted.
    #[test]
    fn messages_sort_newest_first_by_internal_date() {
        let mut storage = SqliteMailStorage::in_memory().unwrap();
        let providers = mock_registry();
        let provider = providers.get(ProviderKind::MockGmail).unwrap();
        let seed =
            tauri::async_runtime::block_on(provider.complete_login("s", "reader@gmail.test"))
                .unwrap();
        storage.add_account(seed, providers.options()).unwrap();

        let messages = storage.bootstrap(providers.options()).messages;
        let dates: Vec<&str> = messages.iter().map(|m| m.received_at.as_str()).collect();
        let mut sorted = dates.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(dates, sorted, "newest first");
    }

    /// Gmail registers when a client config is present, leads the provider grid,
    /// and is the only option that opens a browser. Elm branches on
    /// `browserLogin` to decide whether to show the typed-email form.
    #[test]
    fn a_configured_gmail_provider_leads_the_grid_and_uses_the_browser() {
        let credentials = Arc::new(CredentialStore::new());
        let mut providers = ProviderRegistry::with_mocks();
        providers.insert(Box::new(GmailProvider::new(
            ClientConfig {
                client_id: "test.apps.googleusercontent.com".into(),
                client_secret: "secret".into(),
            },
            credentials,
        )));

        let options = providers.options();
        assert_eq!(options.len(), 3);
        assert_eq!(options[0].display_name, "Gmail");
        assert!(options[0].browser_login);
        assert!(options[1..].iter().all(|option| !option.browser_login));
        assert!(ProviderKind::Gmail.uses_browser_login());
        assert!(!ProviderKind::MockGmail.uses_browser_login());
    }

    /// With no client config the app still starts: Gmail is simply absent, and
    /// the mock providers work as before. A missing setup file is a setup
    /// problem to report on click, not a reason to refuse to launch.
    #[test]
    fn without_a_client_config_gmail_is_absent_and_the_mocks_remain() {
        let providers = ProviderRegistry::with_mocks();
        let options = providers.options();
        assert_eq!(options.len(), 2);
        assert!(options
            .iter()
            .all(|option| option.provider != ProviderKind::Gmail));
        assert!(providers.get(ProviderKind::Gmail).is_err());
    }

    #[test]
    fn gmail_normalization_produces_the_wire_shape_the_ui_expects() {
        let all_labels = vec![
            gmail::api::Label {
                id: "INBOX".into(),
                name: "INBOX".into(),
                label_type: Some("system".into()),
            },
            gmail::api::Label {
                id: "Label_1".into(),
                name: "Work".into(),
                label_type: Some("user".into()),
            },
        ];
        let folders = labels::folders_for_account("gmail-acct", &all_labels);
        let raw = gmail::api::RawMessage {
            id: "abc".into(),
            thread_id: Some("thr".into()),
            label_ids: vec!["INBOX".into(), "UNREAD".into(), "Label_1".into()],
            internal_date: Some("1751378400000".into()),
            raw: Some(base64_encode(
                "From: Ada <ada@example.com>\r\nTo: me@example.com\r\n\
                     Subject: Hello\r\nContent-Type: text/plain\r\n\r\nBody text.\r\n",
            )),
        };

        let message = gmail::normalize("gmail-acct", &folders, &all_labels, &raw).unwrap();
        let json = serde_json::to_value(message.summary()).unwrap();

        // The keys Elm's decoder requires.
        for key in [
            "id",
            "accountId",
            "folderIds",
            "senderName",
            "senderEmail",
            "subject",
            "snippet",
            "receivedAt",
            "unread",
            "starred",
            "labels",
        ] {
            assert!(json.get(key).is_some(), "missing wire key: {key}");
        }
        assert!(json["folderIds"].is_array());
        assert_eq!(json["receivedAt"].as_str().unwrap().len(), 20);
    }
}
