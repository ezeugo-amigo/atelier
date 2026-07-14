use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::AgentKind;

const GLOBAL_WRAPPER_KEYS: &[&str] = &["*", "all"];

#[derive(Debug, Deserialize, Default)]
struct ConfigFile {
    /// Shell command prefixes used to run agent harnesses.
    /// Values are keyed by agent config key, e.g. "claude-code" or "codex".
    #[serde(default)]
    agent_harness_wrappers: HashMap<String, String>,
}

/// Command wrappers configured for agent harness launches in `~/.vigil/config.json`.
#[derive(Debug, Clone, Default)]
pub struct AgentHarnessConfig {
    wrappers: HashMap<String, String>,
}

impl AgentHarnessConfig {
    pub fn load() -> Self {
        let path = config_path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| Self::from_json(&text).ok())
            .unwrap_or_default()
    }

    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        let file: ConfigFile = serde_json::from_str(text)?;
        Ok(Self {
            wrappers: file.agent_harness_wrappers,
        })
    }

    pub fn wrapper_for(&self, agent: AgentKind) -> Option<String> {
        for key in agent_wrapper_keys(agent) {
            if let Some(value) = self.wrappers.get(*key).filter(|v| !v.trim().is_empty()) {
                return Some(value.clone());
            }
        }

        for key in GLOBAL_WRAPPER_KEYS {
            if let Some(value) = self.wrappers.get(*key).filter(|v| !v.trim().is_empty()) {
                return Some(value.clone());
            }
        }

        None
    }
}

pub fn wrap_agent_harness_command(agent: AgentKind, dir: &Path, command: Command) -> Command {
    let Some(wrapper) = AgentHarnessConfig::load().wrapper_for(agent) else {
        return command;
    };
    wrap_command_with_wrapper(agent, dir, command, &wrapper)
}

fn wrap_command_with_wrapper(
    agent: AgentKind,
    dir: &Path,
    command: Command,
    wrapper: &str,
) -> Command {
    if wrapper.trim().is_empty() {
        return command;
    }

    let program = command.get_program().to_os_string();
    let args: Vec<OsString> = command.get_args().map(|a| a.to_os_string()).collect();
    let cwd = command
        .get_current_dir()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| dir.to_path_buf());
    let envs: Vec<(OsString, Option<OsString>)> = command
        .get_envs()
        .map(|(key, value)| (key.to_os_string(), value.map(|v| v.to_os_string())))
        .collect();

    let mut wrapped = Command::new("sh");
    wrapped
        .arg("-c")
        .arg(shell_wrapper_script(wrapper))
        .arg("vigil-agent-harness")
        .arg(program)
        .args(args)
        .current_dir(cwd)
        .env("WORKTREE", dir)
        .env("VIGIL_AGENT", agent.config_key());

    for (key, value) in envs {
        match value {
            Some(value) => {
                wrapped.env(key, value);
            }
            None => {
                wrapped.env_remove(key);
            }
        }
    }

    wrapped
}

fn shell_wrapper_script(wrapper: &str) -> String {
    format!("exec {} \"$@\"\n", wrapper.trim())
}

fn agent_wrapper_keys(agent: AgentKind) -> &'static [&'static str] {
    match agent {
        AgentKind::ClaudeCode => &["claude-code", "claude", "claudecode"],
        AgentKind::Codex => &["codex"],
        AgentKind::Pi => &["pi"],
        AgentKind::OpenCode => &["opencode", "open-code"],
        AgentKind::Droid => &["droid"],
    }
}

fn config_path() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".vigil").join("config.json"))
        .unwrap_or_else(|| PathBuf::from("~/.vigil/config.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_for_prefers_agent_specific_wrapper() {
        let config = AgentHarnessConfig::from_json(
            r#"{
              "agent_harness_wrappers": {
                "*": "ucode",
                "codex": "ucode --codex",
                "claude": "ucode --claude"
              }
            }"#,
        )
        .unwrap();

        assert_eq!(
            config.wrapper_for(AgentKind::Codex),
            Some("ucode --codex".to_string())
        );
        assert_eq!(
            config.wrapper_for(AgentKind::Pi),
            Some("ucode".to_string())
        );
    }

    #[test]
    fn wrapper_runs_configured_command_with_agent_command_as_args() {
        let mut command = Command::new("codex");
        command.arg("resume").arg("abc123").current_dir("/tmp");

        let wrapped =
            wrap_command_with_wrapper(AgentKind::Codex, Path::new("/tmp"), command, "ucode");

        assert_eq!(wrapped.get_program(), "sh");
        let args: Vec<String> = wrapped
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[0], "-c");
        assert_eq!(args[1], "exec ucode \"$@\"\n");
        assert_eq!(args[2], "vigil-agent-harness");
        assert_eq!(args[3], "codex");
        assert_eq!(args[4], "resume");
        assert_eq!(args[5], "abc123");
    }
}
