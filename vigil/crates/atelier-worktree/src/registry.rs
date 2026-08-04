use chrono::{DateTime, Utc};
use serde::{de::Deserializer, Deserialize, Serialize};
use std::path::{Path, PathBuf};
use vigil_core::AgentKind;

/// One repo checked out inside a multi-repo workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoCheckout {
    pub repo_root: PathBuf,
    pub worktree_path: PathBuf,
    pub branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeEntry {
    pub id: String,
    pub agent: AgentKind,
    pub repo_root: PathBuf,
    pub worktree_path: PathBuf,
    pub branch: String,
    pub created_at: DateTime<Utc>,
    /// Checkouts of a multi-repo workspace. Empty for classic single-repo
    /// entries, where the top-level fields fully describe the worktree; then
    /// `worktree_path` is the checkout itself rather than a parent dir.
    /// `skip_serializing_if` keeps pre-existing registry files byte-identical
    /// when rewritten.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repos: Vec<RepoCheckout>,
}

impl WorktreeEntry {
    pub fn is_workspace(&self) -> bool {
        self.repos.len() > 1
    }

    /// Uniform view over single-repo entries and workspaces: legacy entries
    /// synthesize one checkout from the top-level fields.
    pub fn checkouts(&self) -> Vec<RepoCheckout> {
        if self.repos.is_empty() {
            vec![RepoCheckout {
                repo_root: self.repo_root.clone(),
                worktree_path: self.worktree_path.clone(),
                branch: self.branch.clone(),
            }]
        } else {
            self.repos.clone()
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default, deserialize_with = "deserialize_known_worktrees")]
    worktrees: Vec<WorktreeEntry>,
}

fn deserialize_known_worktrees<'de, D>(deserializer: D) -> Result<Vec<WorktreeEntry>, D::Error>
where
    D: Deserializer<'de>,
{
    let values = Vec::<serde_json::Value>::deserialize(deserializer)?;
    let mut entries = Vec::with_capacity(values.len());

    for value in values {
        match serde_json::from_value::<WorktreeEntry>(value.clone()) {
            Ok(entry) => entries.push(entry),
            Err(_) if has_unknown_agent(&value) => {}
            Err(error) => return Err(serde::de::Error::custom(error)),
        }
    }

    Ok(entries)
}

fn has_unknown_agent(value: &serde_json::Value) -> bool {
    value
        .get("agent")
        .is_some_and(|agent| serde_json::from_value::<AgentKind>(agent.clone()).is_err())
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
    #[error("post-create hook failed: {0}")]
    HookFailed(String),
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

    pub fn update_agent(
        &mut self,
        id: &str,
        agent: vigil_core::AgentKind,
    ) -> Result<(), WorktreeError> {
        let entry = self
            .file
            .worktrees
            .iter_mut()
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

#[cfg(test)]
mod tests {
    use super::*;

    const LEGACY_ENTRY: &str = r#"{
        "id": "firm-hilbert",
        "agent": "ClaudeCode",
        "repo_root": "/Users/x/code/platform",
        "worktree_path": "/Users/x/.vigil/worktrees/platform/firm-hilbert",
        "branch": "firm-hilbert",
        "created_at": "2026-01-01T00:00:00Z"
    }"#;

    #[test]
    fn legacy_entry_deserializes_with_empty_repos() {
        let entry: WorktreeEntry = serde_json::from_str(LEGACY_ENTRY).unwrap();
        assert!(entry.repos.is_empty());
        assert!(!entry.is_workspace());
        let checkouts = entry.checkouts();
        assert_eq!(checkouts.len(), 1);
        assert_eq!(checkouts[0].repo_root, entry.repo_root);
        assert_eq!(checkouts[0].worktree_path, entry.worktree_path);
        assert_eq!(checkouts[0].branch, entry.branch);
    }

    #[test]
    fn legacy_entry_serializes_without_repos_key() {
        let entry: WorktreeEntry = serde_json::from_str(LEGACY_ENTRY).unwrap();
        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("\"repos\""));
    }

    #[test]
    fn workspace_entry_round_trips() {
        let mut entry: WorktreeEntry = serde_json::from_str(LEGACY_ENTRY).unwrap();
        entry.worktree_path = PathBuf::from("/Users/x/.vigil/workspaces/firm-hilbert");
        entry.repos = vec![
            RepoCheckout {
                repo_root: PathBuf::from("/Users/x/code/platform"),
                worktree_path: PathBuf::from("/Users/x/.vigil/workspaces/firm-hilbert/platform"),
                branch: "firm-hilbert".into(),
            },
            RepoCheckout {
                repo_root: PathBuf::from("/Users/x/code/console"),
                worktree_path: PathBuf::from("/Users/x/.vigil/workspaces/firm-hilbert/console"),
                branch: "firm-hilbert".into(),
            },
        ];
        let json = serde_json::to_string(&entry).unwrap();
        let back: WorktreeEntry = serde_json::from_str(&json).unwrap();
        assert!(back.is_workspace());
        assert_eq!(back.checkouts().len(), 2);
        assert_eq!(back.repos[1].branch, "firm-hilbert");
    }

    #[test]
    fn unknown_agent_does_not_poison_registry_file() {
        let json = format!(
            r#"{{"worktrees":[{},{{
                "id":"jcode",
                "agent":"Jcode",
                "repo_root":"/Users/x/code/atelier",
                "worktree_path":"/Users/x/.vigil/worktrees/atelier/jcode",
                "branch":"jcode",
                "created_at":"2026-01-01T00:00:00Z"
            }}]}}"#,
            LEGACY_ENTRY
        );

        let file: RegistryFile = serde_json::from_str(&json).unwrap();

        assert_eq!(file.worktrees.len(), 1);
        assert_eq!(file.worktrees[0].id, "firm-hilbert");
    }
}
