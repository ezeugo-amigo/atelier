use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use vigil_core::AgentKind;

use crate::names::generate_name;
use crate::registry::{Registry, RepoCheckout, WorktreeEntry, WorktreeError};

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
    /// Git repo roots. Inferred from CWD via `git rev-parse` if empty.
    /// Two or more roots create a multi-repo workspace.
    pub repo_roots: Vec<PathBuf>,
    /// Explicit worktree directory. Defaults to ~/.vigil/worktrees/<repo>/<name>.
    /// Ignored for multi-repo workspaces.
    pub worktree_dir: Option<PathBuf>,
    /// Skip launching the agent after creating the worktree.
    pub no_launch: bool,
}

/// Create a new worktree: resolve paths, git worktree add, registry append, optional launch.
/// Returns the created entry — caller should print `entry.id` when name was generated.
/// With 2+ repo roots, creates a workspace dir holding one worktree per repo.
pub fn create(
    opts: CreateOptions,
    registry: &mut Registry,
    launch_cmd: Option<Command>,
) -> Result<WorktreeEntry, WorktreeError> {
    let mut repo_roots = opts.repo_roots;
    if repo_roots.is_empty() {
        repo_roots.push(resolve_repo_root()?);
    }
    let mut seen_roots = std::collections::HashSet::new();
    repo_roots.retain(|r| seen_roots.insert(r.clone()));

    // Collect existing names (branches in every repo + registry ids) for collision checking
    let mut existing_names: Vec<String> = repo_roots
        .iter()
        .flat_map(|root| list_git_branches(root).unwrap_or_default())
        .collect();
    existing_names.extend(registry.entries().iter().map(|e| e.id.clone()));

    let name = match opts.name {
        Some(n) => {
            // Multi-repo: fail before creating anything rather than mid-way
            // through the checkout loop. Single-repo keeps git's own error.
            if repo_roots.len() > 1 && existing_names.contains(&n) {
                return Err(WorktreeError::Git(format!(
                    "name '{n}' collides with an existing branch or worktree"
                )));
            }
            n
        }
        None => generate_name(&existing_names, 30)?,
    };

    if repo_roots.len() > 1 {
        return create_workspace(name, opts.agent, repo_roots, registry, opts.no_launch, launch_cmd);
    }
    let repo_root = repo_roots.into_iter().next().unwrap();

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
        repos: vec![],
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

/// Root directory for multi-repo workspaces. A sibling of `worktrees/` so the
/// two layouts (`worktrees/<repo>/<name>` vs `workspaces/<name>/<repo>`) can
/// never collide.
pub fn workspaces_root() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".vigil").join("workspaces"))
        .unwrap_or_else(|| PathBuf::from("~/.vigil/workspaces"))
}

/// Pick a unique subdir name for each repo. Same-named repos (two `api`
/// checkouts) get their parent dir prefixed (`org1-api`), then a numeric
/// suffix as a last resort.
fn dedupe_subdir_names(roots: &[PathBuf]) -> Vec<String> {
    let base_names: Vec<String> = roots
        .iter()
        .map(|r| {
            r.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("repo")
                .to_string()
        })
        .collect();

    let mut taken: Vec<String> = Vec::new();
    roots
        .iter()
        .zip(&base_names)
        .map(|(root, base)| {
            let dupes = base_names.iter().filter(|n| *n == base).count();
            let mut candidate = base.clone();
            if dupes > 1 || taken.contains(&candidate) {
                if let Some(parent) = root
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                {
                    candidate = format!("{parent}-{base}");
                }
            }
            let mut n = 2;
            while taken.contains(&candidate) {
                candidate = format!("{base}-{n}");
                n += 1;
            }
            taken.push(candidate.clone());
            candidate
        })
        .collect()
}

fn write_workspace_manifest(workspace: &Path, name: &str, checkouts: &[RepoCheckout]) {
    let repo_lines: String = checkouts
        .iter()
        .map(|c| {
            format!(
                "- `./{}` — worktree of `{}`\n",
                c.worktree_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?"),
                c.repo_root.display()
            )
        })
        .collect();
    let body = format!(
        "# Workspace `{name}`\n\n\
         This directory is a multi-repo workspace managed by vigil. Each \
         subdirectory is a git worktree, all on branch `{name}`:\n\n\
         {repo_lines}\n\
         Commit and branch operations happen per-repo, inside the respective \
         subdirectory.\n"
    );
    // Best-effort: a missing manifest shouldn't fail creation.
    let _ = std::fs::write(workspace.join("AGENTS.md"), body);
}

/// Create a multi-repo workspace: a parent dir under ~/.vigil/workspaces/<name>
/// holding one worktree per repo, all on branch <name>. The agent launches in
/// the parent dir so it sees every repo.
fn create_workspace(
    name: String,
    agent: AgentKind,
    repo_roots: Vec<PathBuf>,
    registry: &mut Registry,
    no_launch: bool,
    launch_cmd: Option<Command>,
) -> Result<WorktreeEntry, WorktreeError> {
    let workspace = workspaces_root().join(&name);
    std::fs::create_dir_all(&workspace)
        .map_err(|e| WorktreeError::Git(format!("failed to create workspace dir: {e}")))?;

    let subdirs = dedupe_subdir_names(&repo_roots);
    let mut checkouts: Vec<RepoCheckout> = Vec::with_capacity(repo_roots.len());

    for (root, subdir) in repo_roots.iter().zip(&subdirs) {
        let path = workspace.join(subdir);
        if let Err(e) = git_worktree_add(root, &path, &name) {
            // Roll back what we already created so a failed multi-create
            // leaves no half-workspace behind. Best-effort by design.
            for done in &checkouts {
                let _ = Command::new("git")
                    .args(["worktree", "remove", "--force"])
                    .arg(&done.worktree_path)
                    .current_dir(&done.repo_root)
                    .output();
                let _ = Command::new("git")
                    .args(["branch", "-D", &name])
                    .current_dir(&done.repo_root)
                    .output();
            }
            let _ = std::fs::remove_dir_all(&workspace);
            return Err(e);
        }
        checkouts.push(RepoCheckout {
            repo_root: root.clone(),
            worktree_path: path,
            branch: name.clone(),
        });
    }

    write_workspace_manifest(&workspace, &name, &checkouts);

    let entry = WorktreeEntry {
        id: name.clone(),
        agent,
        repo_root: checkouts[0].repo_root.clone(),
        worktree_path: workspace.clone(),
        branch: name,
        created_at: chrono::Utc::now(),
        repos: checkouts,
    };

    registry.append(entry.clone())?;

    for checkout in &entry.repos {
        let hooks = load_hooks_for_repo(&checkout.repo_root);
        run_post_create_hooks(&checkout.worktree_path, &hooks)?;
    }

    if !no_launch {
        if let Some(mut cmd) = launch_cmd {
            cmd.current_dir(&workspace)
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

/// Return true if `git rev-parse --verify <rev>` resolves to a commit.
fn git_rev_exists(repo: &Path, rev: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", rev])
        .current_dir(repo)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Pick the commit a new worktree should branch from.
///
/// Prefers `origin/main` (fetched fresh) when an `origin` remote exists, so
/// worktrees start from the newest upstream main. Falls back to a local `main`
/// branch, then the current `HEAD`, so repos without a remote still work.
fn resolve_base_ref(repo: &Path) -> String {
    // Only talk to origin if it's actually configured.
    let has_origin = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if has_origin {
        // Fetch latest from origin so the worktree starts from the newest main.
        // Use `output()` instead of `status()` so git's progress / worktree
        // messages don't leak into the TUI or CLI screen during normal creation.
        let _ = Command::new("git")
            .args(["fetch", "origin", "main"])
            .current_dir(repo)
            .output();

        if git_rev_exists(repo, "origin/main") {
            return "origin/main".to_string();
        }
    }

    if git_rev_exists(repo, "refs/heads/main") {
        return "main".to_string();
    }

    "HEAD".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_repo_names_pass_through() {
        let roots = vec![
            PathBuf::from("/Users/x/code/platform"),
            PathBuf::from("/Users/x/code/console"),
        ];
        assert_eq!(dedupe_subdir_names(&roots), vec!["platform", "console"]);
    }

    #[test]
    fn same_named_repos_get_parent_prefix() {
        let roots = vec![
            PathBuf::from("/Users/x/org1/api"),
            PathBuf::from("/Users/x/org2/api"),
        ];
        assert_eq!(dedupe_subdir_names(&roots), vec!["org1-api", "org2-api"]);
    }

    #[test]
    fn identical_parents_fall_back_to_numeric_suffix() {
        let roots = vec![PathBuf::from("/a/x/api"), PathBuf::from("/b/x/api")];
        assert_eq!(dedupe_subdir_names(&roots), vec!["x-api", "api-2"]);
    }
}

fn git_worktree_add(repo: &Path, path: &Path, branch: &str) -> Result<(), WorktreeError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| WorktreeError::Git(format!("failed to create parent dir: {e}")))?;
    }

    let base_ref = resolve_base_ref(repo);

    let output = Command::new("git")
        .args(["worktree", "add"])
        .arg(path)
        .arg("-b")
        .arg(branch)
        .arg(&base_ref)
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
