use std::collections::HashSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use vigil_core::SessionId;

#[derive(Debug, Serialize, Deserialize, Default)]
struct ArchiveFile {
    dismissed: HashSet<String>,
}

/// Persists manually-dismissed session IDs to `{data_local_dir}/vigil/archive.json`.
/// Adapter-agnostic: stores plain UUIDs regardless of agent type.
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

    pub fn is_dismissed(&self, id: &SessionId) -> bool {
        self.inner.dismissed.contains(&id.0)
    }

    /// Add a session to the archive and persist immediately.
    pub fn dismiss(&mut self, id: &SessionId) -> std::io::Result<()> {
        self.inner.dismissed.insert(id.0.clone());
        self.save()
    }

    /// Remove a session from the archive and persist immediately.
    pub fn restore(&mut self, id: &SessionId) -> std::io::Result<()> {
        self.inner.dismissed.remove(&id.0);
        self.save()
    }

    fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.inner)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&self.path, json)
    }
}

fn archive_path() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.data_local_dir().join("vigil").join("archive.json"))
        .unwrap_or_else(|| PathBuf::from("~/.local/share/vigil/archive.json"))
}
