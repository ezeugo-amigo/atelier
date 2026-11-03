// Ghost — Tauri desktop shell.
//
// The app is the TypeScript frontend in ../src. Rust opens the window and owns one file:
// every profile, result, and personal-best trace lives in `ghost.json` in the app data
// directory (~/Library/Application Support/ai.atelier.ghost on macOS).

use std::fs;
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("ghost.json"))
}

#[tauri::command]
fn load_db(app: AppHandle) -> Result<Option<String>, String> {
    let path = db_path(&app)?;
    match fs::read_to_string(&path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Writes to a temp file and renames it over the old one, so a crash mid-save can't leave a
/// half-written database behind.
#[tauri::command]
fn save_db(app: AppHandle, data: String) -> Result<(), String> {
    let path = db_path(&app)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, data).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![load_db, save_db])
        .run(tauri::generate_context!())
        .expect("error while running Ghost");
}
