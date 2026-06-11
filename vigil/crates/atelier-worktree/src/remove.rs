use crate::registry::{Registry, WorktreeError};
use std::process::Command;

pub struct RemoveOptions {
    /// Pass --force to git worktree remove and use -D for branch delete.
    pub force: bool,
}

/// Three-step removal: git worktree remove → git branch delete → registry drop.
/// Handles edge cases: missing directory (skip step 1), missing branch (treat as success).
/// Multi-repo workspaces loop both git steps over every checkout, then delete
/// the workspace parent dir. A failure (e.g. one dirty checkout) keeps the
/// registry entry, and re-running is safe: already-removed checkouts skip
/// step 1 and treat their missing branch as success.
pub fn remove(id: &str, opts: RemoveOptions, registry: &mut Registry) -> Result<(), WorktreeError> {
    let entry = registry
        .find_by_id(id)
        .ok_or_else(|| WorktreeError::NotFound(id.to_string()))?
        .clone();

    for checkout in entry.checkouts() {
        // Step 1: remove worktree directory (skip if already gone)
        if checkout.worktree_path.exists() {
            let mut args = vec!["worktree", "remove"];
            if opts.force {
                args.push("--force");
            }
            let status = Command::new("git")
                .args(&args)
                .arg(&checkout.worktree_path)
                .current_dir(&checkout.repo_root)
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
            .args(["branch", branch_flag, &checkout.branch])
            .current_dir(&checkout.repo_root)
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
    }

    // Workspace entries: drop the parent dir (manifest + empty subdir shells).
    // Safety belt: only ever delete under the managed workspaces root.
    if !entry.repos.is_empty()
        && entry.worktree_path.starts_with(crate::create::workspaces_root())
        && entry.worktree_path.exists()
    {
        std::fs::remove_dir_all(&entry.worktree_path)?;
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
