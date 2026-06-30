use serde::Serialize;
use std::sync::Mutex;

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapData {
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

struct MailStore {
    accounts: Vec<Account>,
    folders: Vec<Folder>,
    messages: Vec<MessageDetail>,
    selected_folder_id: String,
    selected_message_id: Option<String>,
    sync_status: SyncStatus,
}

impl MailStore {
    fn mock() -> Self {
        let accounts = vec![
            Account {
                id: "personal".into(),
                display_name: "Personal".into(),
                email_address: "you@example.com".into(),
                provider: "IMAP".into(),
                accent: "#2d6e69".into(),
            },
            Account {
                id: "work".into(),
                display_name: "Work".into(),
                email_address: "you@company.test".into(),
                provider: "JMAP".into(),
                accent: "#9b4e32".into(),
            },
        ];

        let folders = vec![
            folder("inbox", "personal", "Inbox", "inbox"),
            folder("starred", "personal", "Starred", "starred"),
            folder("drafts", "personal", "Drafts", "drafts"),
            folder("sent", "personal", "Sent", "sent"),
            folder("archive", "personal", "Archive", "archive"),
            folder("work-inbox", "work", "Work Inbox", "inbox"),
        ];

        let messages = vec![
            message(
                "m-1001",
                "personal",
                "inbox",
                "Maya Chen",
                "maya@example.net",
                "Dinner plan for Thursday",
                "I booked the table for 7:30. Can you bring the notes from last time?",
                "Today 9:42 AM",
                true,
                true,
                vec!["Personal"],
                vec!["you@example.com"],
                vec![],
                vec![
                    "I booked the table for 7:30 and asked for the quiet corner by the front windows.".into(),
                    "Can you bring the notes from last time? I want to make sure we close the loop on the trip dates before dessert becomes the whole agenda.".into(),
                    "If Thursday gets tight, Friday is still open on my side.".into(),
                ],
            ),
            message(
                "m-1002",
                "work",
                "work-inbox",
                "Platform Alerts",
                "alerts@company.test",
                "API error budget report",
                "The weekly rollup is ready. Two routes crossed the warning threshold.",
                "Today 8:15 AM",
                true,
                false,
                vec!["Work", "Infra"],
                vec!["you@company.test"],
                vec!["sre@company.test"],
                vec![
                    "The weekly rollup is ready. Two routes crossed the warning threshold, both tied to elevated retry traffic during the backfill window.".into(),
                    "Nothing is paging right now, but the trend is worth reviewing before the next batch run. The attached traces point to a small number of slow upstream responses.".into(),
                ],
            ),
            message(
                "m-1003",
                "personal",
                "inbox",
                "Northstar Bank",
                "statements@northstar.example",
                "Your June statement is available",
                "Your new statement has posted. Sign in to review balances and recent activity.",
                "Yesterday",
                false,
                false,
                vec!["Finance"],
                vec!["you@example.com"],
                vec![],
                vec![
                    "Your June statement has posted and is available in online banking.".into(),
                    "For your security, we do not include account details in email. Sign in to review balances, payments, and recent activity.".into(),
                ],
            ),
            message(
                "m-1004",
                "work",
                "work-inbox",
                "Rina Patel",
                "rina@company.test",
                "Design review notes",
                "The navigation changes look solid. I left three small comments on the prototype.",
                "Mon 4:06 PM",
                false,
                true,
                vec!["Work", "Design"],
                vec!["you@company.test"],
                vec![],
                vec![
                    "The navigation changes look solid. I left three small comments on the prototype, mostly around the empty state and density of secondary actions.".into(),
                    "No blocker from me. If you can tighten those labels, I think this is ready for implementation.".into(),
                ],
            ),
            message(
                "m-1005",
                "personal",
                "archive",
                "Receipts",
                "receipts@market.example",
                "Receipt from Green Market",
                "Thanks for shopping with us. Your receipt total was $48.22.",
                "Jun 22",
                false,
                false,
                vec!["Receipts"],
                vec!["you@example.com"],
                vec![],
                vec![
                    "Thanks for shopping with us. Your receipt total was $48.22.".into(),
                    "This message is stored in Archive to keep the inbox focused while preserving the record.".into(),
                ],
            ),
        ];

        let mut store = Self {
            accounts,
            folders,
            messages,
            selected_folder_id: "inbox".into(),
            selected_message_id: Some("m-1001".into()),
            sync_status: SyncStatus {
                state: "Synced".into(),
                last_checked: "Today 9:45 AM".into(),
                detail: "5 messages cached locally".into(),
            },
        };
        store.recalculate_unread();
        store
    }

    fn bootstrap(&self) -> BootstrapData {
        let messages = self.messages_for_folder(&self.selected_folder_id);
        BootstrapData {
            accounts: self.accounts.clone(),
            folders: self.folders.clone(),
            messages,
            selected_folder_id: self.selected_folder_id.clone(),
            selected_message_id: self.selected_message_id.clone(),
            selected_message: self.selected_message(),
            sync_status: self.sync_status.clone(),
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

    fn messages_for_folder(&self, folder_id: &str) -> Vec<MessageSummary> {
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
        if !self.folders.iter().any(|folder| folder.id == folder_id) {
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
        self.set_read_state(message_id, false)?;
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
        self.set_read_state(message_id, !read)?;
        let message = self.message_by_id(message_id)?;
        Ok(MessageUpdate {
            folders: self.folders.clone(),
            message,
            sync_status: self.sync_status.clone(),
        })
    }

    fn archive_message(&mut self, message_id: &str) -> Result<MailboxSnapshot, String> {
        let previous_folder = self.selected_folder_id.clone();
        let message = self
            .messages
            .iter_mut()
            .find(|message| message.id == message_id)
            .ok_or_else(|| format!("Unknown message: {message_id}"))?;

        message.folder_id = "archive".into();
        message.unread = false;
        self.recalculate_unread();

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

    fn refresh(&mut self) -> BootstrapData {
        self.sync_status = SyncStatus {
            state: "Synced".into(),
            last_checked: "Just now".into(),
            detail: format!("{} messages cached locally", self.messages.len()),
        };
        self.bootstrap()
    }

    fn message_by_id(&self, message_id: &str) -> Result<MessageDetail, String> {
        self.messages
            .iter()
            .find(|message| message.id == message_id)
            .cloned()
            .ok_or_else(|| format!("Unknown message: {message_id}"))
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

struct AppState {
    store: Mutex<MailStore>,
}

#[tauri::command]
fn app_bootstrap(state: tauri::State<'_, AppState>) -> Result<BootstrapData, String> {
    let store = state.store.lock().map_err(|error| error.to_string())?;
    Ok(store.bootstrap())
}

#[tauri::command]
fn select_folder(
    state: tauri::State<'_, AppState>,
    folder_id: String,
) -> Result<MailboxSnapshot, String> {
    let mut store = state.store.lock().map_err(|error| error.to_string())?;
    store.select_folder(&folder_id)
}

#[tauri::command]
fn select_message(
    state: tauri::State<'_, AppState>,
    message_id: String,
) -> Result<MessageUpdate, String> {
    let mut store = state.store.lock().map_err(|error| error.to_string())?;
    store.select_message(&message_id)
}

#[tauri::command]
fn search_messages(
    state: tauri::State<'_, AppState>,
    query: String,
) -> Result<MailboxSnapshot, String> {
    let mut store = state.store.lock().map_err(|error| error.to_string())?;
    Ok(store.search(&query))
}

#[tauri::command]
fn mark_message_read(
    state: tauri::State<'_, AppState>,
    message_id: String,
    read: bool,
) -> Result<MessageUpdate, String> {
    let mut store = state.store.lock().map_err(|error| error.to_string())?;
    store.mark_message_read(&message_id, read)
}

#[tauri::command]
fn archive_message(
    state: tauri::State<'_, AppState>,
    message_id: String,
) -> Result<MailboxSnapshot, String> {
    let mut store = state.store.lock().map_err(|error| error.to_string())?;
    store.archive_message(&message_id)
}

#[tauri::command]
fn refresh_mail(state: tauri::State<'_, AppState>) -> Result<BootstrapData, String> {
    let mut store = state.store.lock().map_err(|error| error.to_string())?;
    Ok(store.refresh())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            store: Mutex::new(MailStore::mock()),
        })
        .invoke_handler(tauri::generate_handler![
            app_bootstrap,
            select_folder,
            select_message,
            search_messages,
            mark_message_read,
            archive_message,
            refresh_mail
        ])
        .run(tauri::generate_context!())
        .expect("error while running Maildesk");
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
