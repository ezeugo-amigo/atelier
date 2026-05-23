use crate::registry::{Registry, WorktreeError};
use std::process::Command;

pub struct RemoveOptions {
    /// Pass --force to git worktree remove and use -D for branch delete.
    pub force: bool,
}

/// Three-step removal: git worktree remove → git branch delete → registry drop.
/// Handles edge cases: missing directory (skip step 1), missing branch (treat as success).
pub fn remove(id: &str, opts: RemoveOptions, registry: &mut Registry) -> Result<(), WorktreeError> {
    let entry = registry
        .find_by_id(id)
        .ok_or_else(|| WorktreeError::NotFound(id.to_string()))?
        .clone();

    // Step 1: remove worktree directory (skip if already gone)
    if entry.worktree_path.exists() {
        let mut args = vec!["worktree", "remove"];
        if opts.force {
            args.push("--force");
        }
        let status = Command::new("git")
            .args(&args)
            .arg(&entry.worktree_path)
            .current_dir(&entry.repo_root)
            .status()?;
        if !status.success() {
            return Err(WorktreeError::Git(
                "git worktree remove refused — uncommitted changes? Use --force to override."
                    .into(),
            ));
        }
    }

    // Step 2: delete branch (treat "not found" as success)
    let branch_flag = if opts.force { "-D" } else { "-d" };
    let out = Command::new("git")
        .args(["branch", branch_flag, &entry.branch])
        .current_dir(&entry.repo_root)
        .output()?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let is_missing = stderr.contains("not found")
            || stderr.contains("no branch named")
            || stderr.contains("error: branch");
        if !is_missing {
            return Err(WorktreeError::Git(format!(
                "git branch {branch_flag} failed: {}",
                stderr.trim()
            )));
        }
    }

    // Step 3: drop from registry
    registry.remove_id(id)
}

/// Remove all registry entries whose worktree_path no longer exists on disk.
/// Does not touch git — the directory is already gone.
pub fn prune(registry: &mut Registry) -> Result<Vec<String>, WorktreeError> {
    let stale: Vec<String> = registry
        .entries()
        .iter()
        .filter(|e| !e.worktree_path.exists())
        .map(|e| e.id.clone())
        .collect();
    for id in &stale {
        registry.remove_id(id)?;
    }
    Ok(stale)
}
