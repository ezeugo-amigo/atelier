use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use vigil_core::AgentKind;

use crate::names::generate_name;
use crate::registry::{Registry, WorktreeEntry, WorktreeError};

#[derive(serde::Deserialize, Default)]
struct HooksConfig {
    #[serde(default)]
    worktree_hooks: HashMap<String, Vec<String>>,
}

fn load_hooks_for_repo(repo_root: &Path) -> Vec<String> {
    let config_path = directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".vigil").join("config.json"))
        .unwrap_or_else(|| PathBuf::from("~/.vigil/config.json"));

    let text = match std::fs::read_to_string(&config_path) {
        Ok(t) => t,
        Err(_) => return vec![],
    };
    let cfg: HooksConfig = match serde_json::from_str(&text) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    // Match by full path first, then by repo directory name.
    let full_key = repo_root.to_string_lossy().to_string();
    let name_key = repo_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    cfg.worktree_hooks
        .get(&full_key)
        .or_else(|| cfg.worktree_hooks.get(&name_key))
        .cloned()
        .unwrap_or_default()
}

fn run_post_create_hooks(worktree_path: &Path, hooks: &[String]) -> Result<(), WorktreeError> {
    for cmd in hooks {
        let output = Command::new("sh")
            .args(["-c", cmd])
            .env("WORKTREE", worktree_path)
            .output()
            .map_err(|e| WorktreeError::HookFailed(format!("{cmd:?}: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let detail = match (stdout.is_empty(), stderr.is_empty()) {
                (true, true) => String::new(),
                (false, true) => format!("\n{stdout}"),
                (true, false) => format!("\n{stderr}"),
                (false, false) => format!("\n{stderr}\n{stdout}"),
            };
            return Err(WorktreeError::HookFailed(format!("`{cmd}`{detail}")));
        }
    }
    Ok(())
}

pub struct CreateOptions {
    /// Worktree name (and branch name). Generated if None.
    pub name: Option<String>,
    pub agent: AgentKind,
    /// Git repo root. Inferred from CWD via `git rev-parse` if None.
    pub repo_root: Option<PathBuf>,
    /// Explicit worktree directory. Defaults to ~/.vigil/worktrees/<repo>/<name>.
    pub worktree_dir: Option<PathBuf>,
    /// Skip launching the agent after creating the worktree.
    pub no_launch: bool,
}

/// Create a new worktree: resolve paths, git worktree add, registry append, optional launch.
/// Returns the created entry — caller should print `entry.id` when name was generated.
pub fn create(
    opts: CreateOptions,
    registry: &mut Registry,
    launch_cmd: Option<Command>,
) -> Result<WorktreeEntry, WorktreeError> {
    let repo_root = match opts.repo_root {
        Some(p) => p,
        None => resolve_repo_root()?,
    };

    // Collect existing names (branches + registry ids) for collision checking
    let existing_branches = list_git_branches(&repo_root).unwrap_or_default();
    let existing_ids: Vec<String> = registry.entries().iter().map(|e| e.id.clone()).collect();
    let mut existing_names = existing_branches;
    existing_names.extend(existing_ids);

    let name = match opts.name {
        Some(n) => n,
        None => generate_name(&existing_names, 30)?,
    };

    let repo_name = repo_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("repo");

    let worktree_path = opts.worktree_dir.unwrap_or_else(|| {
        directories::BaseDirs::new()
            .map(|b| {
                b.home_dir()
                    .join(".vigil")
                    .join("worktrees")
                    .join(repo_name)
                    .join(&name)
            })
            .unwrap_or_else(|| PathBuf::from(format!("~/.vigil/worktrees/{repo_name}/{name}")))
    });

    git_worktree_add(&repo_root, &worktree_path, &name)?;

    let entry = WorktreeEntry {
        id: name.clone(),
        agent: opts.agent,
        repo_root,
        worktree_path: worktree_path.clone(),
        branch: name,
        created_at: chrono::Utc::now(),
    };

    registry.append(entry.clone())?;

    let hooks = load_hooks_for_repo(&entry.repo_root);
    run_post_create_hooks(&worktree_path, &hooks)?;

    if !opts.no_launch {
        if let Some(mut cmd) = launch_cmd {
            cmd.current_dir(&worktree_path)
                .spawn()
                .map_err(|e| WorktreeError::Git(format!("failed to launch agent: {e}")))?;
        }
    }

    Ok(entry)
}

pub fn resolve_repo_root() -> Result<PathBuf, WorktreeError> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|_| WorktreeError::NotARepo)?;
    if !out.status.success() {
        return Err(WorktreeError::NotARepo);
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(PathBuf::from(s))
}

fn list_git_branches(repo_root: &Path) -> Result<Vec<String>, WorktreeError> {
    let out = Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(repo_root)
        .output()?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

fn git_output_details(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!(": {stdout}"),
        (true, false) => format!(": {stderr}"),
        (false, false) => format!(": {stderr}\n{stdout}"),
    }
}

fn git_worktree_add(repo: &Path, path: &Path, branch: &str) -> Result<(), WorktreeError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| WorktreeError::Git(format!("failed to create parent dir: {e}")))?;
    }

    // Fetch latest from origin so the worktree starts from the newest main.
    // Use `output()` instead of `status()` so git's progress / worktree messages
    // don't leak into the TUI or CLI screen during normal creation.
    let _ = Command::new("git")
        .args(["fetch", "origin", "main"])
        .current_dir(repo)
        .output();

    let output = Command::new("git")
        .args(["worktree", "add"])
        .arg(path)
        .arg("-b")
        .arg(branch)
        .arg("origin/main")
        .current_dir(repo)
        .output()?;
    if !output.status.success() {
        let details = git_output_details(&output.stdout, &output.stderr);
        return Err(WorktreeError::Git(format!(
            "git worktree add failed for branch '{branch}'{details}"
        )));
    }
    Ok(())
}
