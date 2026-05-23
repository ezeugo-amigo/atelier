use std::collections::HashMap;
use std::time::Instant;

use anyhow::Result;
use atelier_worktree::Registry;
use clap::{Parser, Subcommand};
use vigil_adapter_claude::ClaudeCodeAdapter;
use vigil_core::{AgentAdapter, SessionState};

#[derive(Parser)]
#[command(name = "vigil-probe", about = "Probe registered containers")]
struct Args {
    #[command(subcommand)]
    command: Option<Cmd>,

    /// Filter to a specific container id
    #[arg(long, global = true)]
    id: Option<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// List current state of all containers (default)
    List,
    /// Watch state transitions in real-time
    Trace,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    let adapter = ClaudeCodeAdapter::new()?;

    match args.command.unwrap_or(Cmd::List) {
        Cmd::List => list(&adapter, args.id.as_deref()).await?,
        Cmd::Trace => trace(&adapter, args.id.as_deref()).await?,
    }

    Ok(())
}

async fn load_entries(filter: Option<&str>) -> Result<Vec<atelier_worktree::WorktreeEntry>> {
    let registry = Registry::load()?;
    let entries: Vec<_> = registry
        .entries()
        .iter()
        .filter(|e| filter.map_or(true, |f| e.id == f))
        .cloned()
        .collect();
    if entries.is_empty() {
        if let Some(f) = filter {
            eprintln!("No container found with id '{f}'.");
        } else {
            eprintln!("No containers registered. Run `atelier wt new` to create one.");
        }
    }
    Ok(entries)
}

async fn list(adapter: &ClaudeCodeAdapter, filter: Option<&str>) -> Result<()> {
    let entries = load_entries(filter).await?;
    if entries.is_empty() {
        return Ok(());
    }

    println!(
        "{:<22}  {:<12}  {:<30}  {}",
        "CONTAINER", "STATE", "LAST MESSAGE", "PATH"
    );
    println!("{}", "-".repeat(110));

    for entry in entries {
        let probe = adapter.probe(&entry.worktree_path).await;
        let state = fmt_state(&probe.state);
        let msg = probe
            .last_user_message
            .as_deref()
            .unwrap_or("-")
            .chars()
            .take(30)
            .collect::<String>();
        let path = entry.worktree_path.display().to_string();
        println!("{:<22}  {:<12}  {:<30}  {}", entry.id, state, msg, path);
    }

    Ok(())
}

async fn trace(adapter: &ClaudeCodeAdapter, filter: Option<&str>) -> Result<()> {
    let mut seen: HashMap<String, (SessionState, Instant)> = HashMap::new();
    let mut tick: u64 = 0;

    println!(
        "{:<8}  {:<22}  {:<6}  {:<8}  {:<6}  {}",
        "TICK", "CONTAINER", "IDLE_S", "HELD", "→", "STATE"
    );
    println!("{}", "-".repeat(90));

    loop {
        let entries = load_entries(filter).await?;

        for entry in &entries {
            let probe = adapter.probe(&entry.worktree_path).await;
            let state = probe.state;

            let prev = seen.get(&entry.id);
            let changed = prev.map(|(s, _)| s != &state).unwrap_or(true);

            if changed {
                let duration = prev
                    .map(|(s, t)| {
                        format!(" [{:.1}s in {}]", t.elapsed().as_secs_f64(), fmt_state(s))
                    })
                    .unwrap_or_default();

                let arrow = if prev.is_some() { "→" } else { " " };
                println!(
                    "{:<8}  {:<22}  {}  {}  {}",
                    tick,
                    entry.id,
                    arrow,
                    fmt_state(&state),
                    duration
                );

                seen.insert(entry.id.clone(), (state, Instant::now()));
            }
        }

        tick += 1;
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}

fn fmt_state(s: &SessionState) -> String {
    match s {
        SessionState::NoSession => "no-session".into(),
        SessionState::Running => "running".into(),
        SessionState::AwaitingInput { reason: Some(r) } => format!("awaiting({r})"),
        SessionState::AwaitingInput { reason: None } => "awaiting".into(),
        SessionState::Idle => "idle".into(),
        SessionState::Done => "done".into(),
        SessionState::Error { message: m } => format!("error({m})"),
        SessionState::Unknown => "unknown".into(),
    }
}
