use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use vigil_core::AgentKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScratchChat {
    pub id: String,
    pub title: String,
    pub workdir: PathBuf,
    pub agent: AgentKind,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ScratchFile {
    chats: Vec<ScratchChat>,
}

pub struct ScratchStore {
    path: PathBuf,
    inner: ScratchFile,
}

impl ScratchStore {
    pub fn load() -> Self {
        let path = scratch_file_path();
        let inner = std::fs::read_to_string(&path)
            .ok()
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_default();
        Self { path, inner }
    }

    pub fn chats(&self) -> &[ScratchChat] {
        &self.inner.chats
    }

    pub fn create(&mut self, title: String, agent: AgentKind) -> std::io::Result<ScratchChat> {
        let root = scratch_root();
        std::fs::create_dir_all(&root)?;

        let timestamp = Utc::now().timestamp_millis();
        let mut id = format!("scratch-{timestamp}");
        let mut suffix = 2;
        while self.inner.chats.iter().any(|chat| chat.id == id) {
            id = format!("scratch-{timestamp}-{suffix}");
            suffix += 1;
        }

        let workdir = root.join(&id);
        std::fs::create_dir_all(&workdir)?;

        let title = if title.trim().is_empty() {
            "Untitled chat".to_string()
        } else {
            title.trim().to_string()
        };
        let chat = ScratchChat {
            id,
            title,
            workdir,
            agent,
            created_at: Utc::now(),
        };
        self.inner.chats.push(chat.clone());
        self.save()?;
        Ok(chat)
    }

    pub fn set_agent(&mut self, id: &str, agent: AgentKind) -> std::io::Result<()> {
        if let Some(chat) = self.inner.chats.iter_mut().find(|chat| chat.id == id) {
            chat.agent = agent;
            self.save()?;
        }
        Ok(())
    }

    pub fn delete(&mut self, id: &str) -> std::io::Result<()> {
        let Some(index) = self.inner.chats.iter().position(|chat| chat.id == id) else {
            return Ok(());
        };
        let chat = self.inner.chats.remove(index);
        self.save()?;
        match std::fs::remove_dir_all(&chat.workdir) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.inner).map_err(std::io::Error::other)?;
        std::fs::write(&self.path, json)
    }
}

fn scratch_root() -> PathBuf {
    directories::BaseDirs::new()
        .map(|base| base.data_local_dir().join("vigil").join("chats"))
        .unwrap_or_else(|| PathBuf::from("~/.local/share/vigil/chats"))
}

fn scratch_file_path() -> PathBuf {
    scratch_root().join("chats.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scratch_ids_are_namespaced() {
        let timestamp = Utc::now().timestamp_millis();
        assert!(format!("scratch-{timestamp}").starts_with("scratch-"));
    }
}
