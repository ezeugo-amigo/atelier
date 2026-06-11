use anyhow::{Context, Result};
use atelier_worktree::{create, prune, remove, CreateOptions, Registry, RemoveOptions};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use vigil_core::AgentKind;

#[derive(Parser)]
#[command(name = "atelier", about = "Atelier agent tooling")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Worktree management
    Wt {
        #[command(subcommand)]
        action: WtAction,
    },
}

#[derive(Subcommand)]
enum WtAction {
    /// Create a new worktree and launch an agent in it
    New {
        /// Worktree name (and branch name). Auto-generated if omitted.
        name: Option<String>,
        /// Agent to launch (claude, codex, pi, opencode) [default: claude]
        #[arg(long, short, default_value = "claude")]
        agent: String,
        /// Git repository root. Inferred from CWD if omitted. Repeat the flag
        /// to create a multi-repo workspace (one worktree per repo, shared
        /// branch, agent launched in the parent dir).
        #[arg(long = "repo")]
        repos: Vec<PathBuf>,
        /// Skip launching the agent after creation.
        #[arg(long)]
        no_launch: bool,
    },
    /// List all registered worktrees
    List,
    /// Remove a worktree and its branch
    Remove {
        /// Worktree ID (name)
        id: String,
        /// Force removal even with uncommitted changes
        #[arg(long, short)]
        force: bool,
    },
    /// Remove registry entries whose directories no longer exist on disk
    Prune,
}

fn parse_agent(s: &str) -> Result<AgentKind> {
    match s.to_lowercase().as_str() {
        "claude" | "claude-code" | "claudecode" => Ok(AgentKind::ClaudeCode),
        "codex" => Ok(AgentKind::Codex),
        "pi" => Ok(AgentKind::Pi),
        "opencode" => Ok(AgentKind::OpenCode),
        "droid" => Ok(AgentKind::Droid),
        other => {
            anyhow::bail!("unknown agent '{other}'. Valid: claude, codex, pi, opencode, droid")
        }
    }
}

fn agent_bin(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::ClaudeCode => "claude",
        AgentKind::Codex => "codex",
        AgentKind::Pi => "pi",
        AgentKind::OpenCode => "opencode",
        AgentKind::Droid => "droid",
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Wt { action } => handle_wt(action),
    }
}

fn handle_wt(action: WtAction) -> Result<()> {
    match action {
        WtAction::New {
            name,
            agent,
            repos,
            no_launch,
        } => {
            let agent_kind = parse_agent(&agent)?;
            let mut registry = Registry::load().context("failed to load registry")?;

            let launch_cmd = Some(std::process::Command::new(agent_bin(agent_kind)));

            let opts = CreateOptions {
                name,
                agent: agent_kind,
                repo_roots: repos,
                worktree_dir: None,
                no_launch,
            };

            let entry =
                create(opts, &mut registry, launch_cmd).context("failed to create worktree")?;

            let kind = if entry.is_workspace() {
                "workspace"
            } else {
                "worktree"
            };
            println!("Created {kind} '{}'", entry.id);
            println!("  path:   {}", entry.worktree_path.display());
            for checkout in &entry.repos {
                println!("    - {}", checkout.worktree_path.display());
            }
            println!("  branch: {}", entry.branch);
            println!("  agent:  {}", entry.agent.display_name());
            if !no_launch {
                println!("  launched agent in {kind} directory");
            }
            Ok(())
        }

        WtAction::List => {
            let registry = Registry::load().context("failed to load registry")?;
            let entries = registry.entries();
            if entries.is_empty() {
                println!("No worktrees registered.");
                return Ok(());
            }
            println!("{:<30} {:<12} {:<50} {}", "ID", "AGENT", "PATH", "CREATED");
            for e in entries {
                println!(
                    "{:<30} {:<12} {:<50} {}",
                    e.id,
                    e.agent.display_name(),
                    e.worktree_path.display(),
                    e.created_at.format("%Y-%m-%d %H:%M"),
                );
            }
            Ok(())
        }

        WtAction::Remove { id, force } => {
            let mut registry = Registry::load().context("failed to load registry")?;
            remove(&id, RemoveOptions { force }, &mut registry)
                .context("failed to remove worktree")?;
            println!("Removed worktree '{id}'");
            Ok(())
        }

        WtAction::Prune => {
            let mut registry = Registry::load().context("failed to load registry")?;
            let pruned = prune(&mut registry).context("failed to prune registry")?;
            if pruned.is_empty() {
                println!("Nothing to prune.");
            } else {
                let s = if pruned.len() == 1 { "y" } else { "ies" };
                println!("Pruned {} entr{s}:", pruned.len());
                for id in &pruned {
                    println!("  - {id}");
                }
            }
            Ok(())
        }
    }
}
