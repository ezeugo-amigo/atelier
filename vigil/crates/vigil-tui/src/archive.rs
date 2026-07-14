use std::collections::HashSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
struct ArchiveFile {
    dismissed: HashSet<String>,
}

/// Persists manually-dismissed container IDs to `{data_local_dir}/vigil/archive.json`.
pub struct Archive {
    path: PathBuf,
    inner: ArchiveFile,
}

impl Archive {
    pub fn load() -> Self {
        let path = archive_path();
        let inner = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, inner }
    }

    pub fn is_dismissed_id(&self, id: &str) -> bool {
        self.inner.dismissed.contains(id)
    }

    pub fn dismiss_id(&mut self, id: &str) -> std::io::Result<()> {
        self.inner.dismissed.insert(id.to_string());
        self.save()
    }

    pub fn restore_id(&mut self, id: &str) -> std::io::Result<()> {
        self.inner.dismissed.remove(id);
        self.save()
    }

    fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.inner).map_err(std::io::Error::other)?;
        std::fs::write(&self.path, json)
    }
}

fn archive_path() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.data_local_dir().join("vigil").join("archive.json"))
        .unwrap_or_else(|| PathBuf::from("~/.local/share/vigil/archive.json"))
}
