// Today — Tauri desktop shell.
//
// The shell is intentionally thin: the entire app is the Elm frontend in
// ../web, and persistence lives in the webview's own IndexedDB (see web/db.js).
// There are no custom Tauri commands — Rust just opens the window and serves
// the bundled frontend.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running Today");
}
