use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use vigil_core::AgentKind;

#[derive(Debug, Serialize, Deserialize)]
struct ConfigFile {
    /// Directories to search for git repositories (supports `~` expansion).
    search_paths: Vec<String>,
    /// Shell commands to run after creating a worktree, keyed by repo name or full repo path.
    /// Each command is run via `sh -c` with `$WORKTREE` set to the new worktree's absolute path.
    #[serde(default)]
    worktree_hooks: HashMap<String, Vec<String>>,
    /// Default agent for new containers. Accepts "claude-code", "pi", or "droid".
    #[serde(default = "default_agent_string")]
    default_agent: String,
}

fn default_agent_string() -> String {
    "claude-code".into()
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            search_paths: vec!["~/code".into(), "~/projects".into()],
            worktree_hooks: HashMap::new(),
            default_agent: default_agent_string(),
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

    /// Default agent kind for newly created containers.
    pub fn default_agent(&self) -> AgentKind {
        parse_agent(&self.inner.default_agent).unwrap_or(AgentKind::ClaudeCode)
    }

    /// Persist a new default agent to `~/.vigil/config.json`, preserving other fields.
    pub fn set_default_agent(agent: AgentKind) -> std::io::Result<()> {
        let path = config_path();
        let mut inner = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<ConfigFile>(&s).ok())
            .unwrap_or_default();
        inner.default_agent = agent_config_string(agent).into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&inner).map_err(std::io::Error::other)?;
        std::fs::write(&path, json)
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

fn agent_config_string(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::ClaudeCode => "claude-code",
        AgentKind::Codex => "codex",
        AgentKind::Pi => "pi",
        AgentKind::OpenCode => "opencode",
        AgentKind::Droid => "droid",
    }
}

fn parse_agent(s: &str) -> Option<AgentKind> {
    match s.trim().to_ascii_lowercase().as_str() {
        "claude" | "claude-code" | "claudecode" => Some(AgentKind::ClaudeCode),
        "codex" => Some(AgentKind::Codex),
        "pi" => Some(AgentKind::Pi),
        "opencode" => Some(AgentKind::OpenCode),
        "droid" => Some(AgentKind::Droid),
        _ => None,
    }
}

fn config_path() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".vigil").join("config.json"))
        .unwrap_or_else(|| PathBuf::from("~/.vigil/config.json"))
}
