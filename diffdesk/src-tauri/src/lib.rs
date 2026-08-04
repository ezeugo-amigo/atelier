use diffdesk_core::{
    app_session_id_from_env_or_args, cancel_review as core_cancel_review,
    flush_review_state as core_flush_review_state, help_text, load_drafts,
    load_review_state as core_load_review_state, load_session as core_load_session,
    read_input_diff, save_drafts as core_save_drafts, save_file_review as core_save_file_review,
    submit_review as core_submit_review, DraftFile, ReviewComment, ReviewStateFile, SessionFile,
    SubmitPayload, SubmitResult,
};
use serde::Serialize;
use std::env;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, WindowEvent};

struct AppState {
    session_id: Mutex<String>,
    completed: Mutex<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LoadSessionResponse {
    session: SessionFile,
    raw_diff: String,
    drafts: Option<DraftFile>,
}

#[tauri::command]
fn current_session_id(state: tauri::State<'_, AppState>) -> Result<String, String> {
    state
        .session_id
        .lock()
        .map(|value| value.clone())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn load_session(session_id: String) -> Result<LoadSessionResponse, String> {
    let session = core_load_session(&session_id).map_err(|error| error.to_string())?;
    let raw_diff = read_input_diff(&session_id).map_err(|error| error.to_string())?;
    let drafts = load_drafts(&session_id).map_err(|error| error.to_string())?;
    Ok(LoadSessionResponse {
        session,
        raw_diff,
        drafts,
    })
}

#[tauri::command]
fn load_review_state() -> Result<ReviewStateFile, String> {
    core_load_review_state().map_err(|error| error.to_string())
}

#[tauri::command]
fn save_file_review(
    source_key: String,
    file_key: String,
    fingerprint: String,
    reviewed: bool,
) -> Result<(), String> {
    core_save_file_review(&source_key, &file_key, &fingerprint, reviewed)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn save_drafts(
    session_id: String,
    summary: String,
    comments: Vec<ReviewComment>,
) -> Result<DraftFile, String> {
    core_save_drafts(&session_id, summary, comments).map_err(|error| error.to_string())
}

#[tauri::command]
fn submit_review(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    session_id: String,
    payload: SubmitPayload,
) -> Result<SubmitResult, String> {
    let result = core_submit_review(&session_id, payload).map_err(|error| error.to_string())?;
    let _ = core_flush_review_state();
    if let Ok(mut completed) = state.completed.lock() {
        *completed = true;
    }
    app.exit(0);
    Ok(result)
}

#[tauri::command]
fn cancel_review(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    core_cancel_review(&session_id).map_err(|error| error.to_string())?;
    let _ = core_flush_review_state();
    if let Ok(mut completed) = state.completed.lock() {
        *completed = true;
    }
    app.exit(1);
    Ok(())
}

pub fn run() {
    let raw_args = env::args().skip(1).collect::<Vec<_>>();
    if raw_args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", help_text());
        return;
    }

    let current_dir = env::current_dir().unwrap_or_else(|_| dirs::home_dir().unwrap_or_default());
    let session = match app_session_id_from_env_or_args(&raw_args, &current_dir) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("diffdesk-app: {error:#}");
            std::process::exit(2);
        }
    };
    let session_id = session.session_id.clone();

    tauri::Builder::default()
        .manage(AppState {
            session_id: Mutex::new(session_id),
            completed: Mutex::new(false),
        })
        .invoke_handler(tauri::generate_handler![
            current_session_id,
            load_session,
            load_review_state,
            save_file_review,
            save_drafts,
            submit_review,
            cancel_review
        ])
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title("Diffdesk");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::CloseRequested { .. }) {
                let _ = core_flush_review_state();
                let state = window.state::<AppState>();
                let completed = state.completed.lock().map(|value| *value).unwrap_or(false);
                if !completed {
                    if let Ok(session_id) = state.session_id.lock() {
                        let _ = core_cancel_review(&session_id);
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Diffdesk");
}
