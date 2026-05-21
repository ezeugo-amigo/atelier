use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use vigil_core::AgentKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeEntry {
    pub id: String,
    pub agent: AgentKind,
    pub repo_root: PathBuf,
    pub worktree_path: PathBuf,
    pub branch: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default)]
    worktrees: Vec<WorktreeEntry>,
}

#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("worktree not found: {0}")]
    NotFound(String),
    #[error("git command failed: {0}")]
    Git(String),
    #[error("not inside a git repository")]
    NotARepo,
    #[error("name collision: could not generate unique name after {0} attempts")]
    NameCollision(usize),
}

pub struct Registry {
    path: PathBuf,
    file: RegistryFile,
}

impl Registry {
    pub fn path() -> PathBuf {
        directories::BaseDirs::new()
            .map(|b| b.home_dir().join(".vigil").join("worktrees.json"))
            .unwrap_or_else(|| PathBuf::from("~/.vigil/worktrees.json"))
    }

    pub fn load() -> Result<Self, WorktreeError> {
        let path = Self::path();
        let file = if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            serde_json::from_str(&text)?
        } else {
            RegistryFile::default()
        };
        Ok(Self { path, file })
    }

    pub fn entries(&self) -> &[WorktreeEntry] {
        &self.file.worktrees
    }

    pub fn find_by_id(&self, id: &str) -> Option<&WorktreeEntry> {
        self.file.worktrees.iter().find(|e| e.id == id)
    }

    pub fn find_by_path(&self, path: &Path) -> Option<&WorktreeEntry> {
        self.file.worktrees.iter().find(|e| e.worktree_path == path)
    }

    pub fn append(&mut self, entry: WorktreeEntry) -> Result<(), WorktreeError> {
        self.file.worktrees.push(entry);
        self.save()
    }

    pub fn update_agent(&mut self, id: &str, agent: vigil_core::AgentKind) -> Result<(), WorktreeError> {
        let entry = self.file.worktrees.iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| WorktreeError::NotFound(id.to_string()))?;
        entry.agent = agent;
        self.save()
    }

    pub fn remove_id(&mut self, id: &str) -> Result<(), WorktreeError> {
        let before = self.file.worktrees.len();
        self.file.worktrees.retain(|e| e.id != id);
        if self.file.worktrees.len() == before {
            return Err(WorktreeError::NotFound(id.to_string()));
        }
        self.save()
    }

    fn save(&self) -> Result<(), WorktreeError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.file)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }
}
