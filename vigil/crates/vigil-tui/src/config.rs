use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct ConfigFile {
    /// Directories to search for git repositories (supports `~` expansion).
    search_paths: Vec<String>,
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            search_paths: vec!["~/code".into(), "~/projects".into()],
        }
    }
}

/// Application configuration persisted at `~/.vigil/config.json`.
pub struct Config {
    inner: ConfigFile,
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        let inner = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| {
                let default = ConfigFile::default();
                // Write defaults so the user can discover and edit the file.
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ =
                    serde_json::to_string_pretty(&default).map(|json| std::fs::write(&path, json));
                default
            });
        Self { inner }
    }

    /// Returns resolved absolute paths for each configured search directory.
    pub fn search_paths(&self) -> Vec<PathBuf> {
        let home = std::env::var("HOME").unwrap_or_default();
        self.inner
            .search_paths
            .iter()
            .filter_map(|raw| {
                let expanded = if raw.starts_with("~/") {
                    format!("{}/{}", home, &raw[2..])
                } else if raw == "~" {
                    home.clone()
                } else {
                    raw.clone()
                };
                let p = PathBuf::from(expanded);
                if p.is_dir() {
                    Some(p)
                } else {
                    None
                }
            })
            .collect()
    }
}

fn config_path() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".vigil").join("config.json"))
        .unwrap_or_else(|| PathBuf::from("~/.vigil/config.json"))
}
