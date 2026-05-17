use std::collections::HashMap;
use std::time::Instant;

use anyhow::Result;
use clap::{Parser, Subcommand};
use vigil_adapter_claude::ClaudeCodeAdapter;
use vigil_core::{AgentAdapter, SessionId, SessionState};

#[derive(Parser)]
#[command(name = "vigil-probe", about = "Probe Claude Code sessions")]
struct Args {
    #[command(subcommand)]
    command: Option<Cmd>,

    /// Filter to a specific session UUID
    #[arg(long, global = true)]
    session: Option<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// List current state of all sessions (default)
    List,
    /// Watch state transitions in real-time, polling every second
    Trace,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Only enable tracing for explicit RUST_LOG — keeps trace output clean by default
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    let adapter = ClaudeCodeAdapter::new()?;

    match args.command.unwrap_or(Cmd::List) {
        Cmd::List => list(&adapter, args.session.as_deref()).await?,
        Cmd::Trace => trace(&adapter, args.session.as_deref()).await?,
    }

    Ok(())
}

// ── list ─────────────────────────────────────────────────────────────────────

async fn list(adapter: &ClaudeCodeAdapter, filter: Option<&str>) -> Result<()> {
    let ids = resolve_ids(adapter, filter).await?;
    if ids.is_empty() { return Ok(()); }

    println!("{:<40}  {:<35}  {:<35}  {}",
        "SESSION ID", "STATE", "PROJECT", "LAST MESSAGE");
    println!("{}", "-".repeat(155));

    for id in ids {
        match adapter.read(&id).await {
            Ok(s) => {
                let state = fmt_state(&s.state);
                let project = s.project_dir.as_deref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "-".into());
                let msg = s.last_user_message.as_deref().unwrap_or("-")
                    .chars().take(55).collect::<String>();
                println!("{:<40}  {:<35}  {:<35}  {}", id, state, project, msg);
            }
            Err(e) => println!("{:<40}  ERROR: {}", id, e),
        }
    }
    Ok(())
}

// ── trace ─────────────────────────────────────────────────────────────────────

struct TraceEntry {
    state: SessionState,
    entered_at: Instant,
    tick: u64,
}

async fn trace(adapter: &ClaudeCodeAdapter, filter: Option<&str>) -> Result<()> {
    let mut seen: HashMap<SessionId, TraceEntry> = HashMap::new();
    let mut tick: u64 = 0;

    println!("{:<8}  {:<40}  {:<6}  {:<8}  {:<6}  {}",
        "TICK", "SESSION", "IDLE_S", "HELD", "→", "STATE");
    println!("{}", "-".repeat(100));

    loop {
        let ids = resolve_ids(adapter, filter).await?;

        for id in &ids {
            match adapter.sample_raw(id).await {
                Ok((log, fs, state)) => {
                    let secs_idle = fs.secs_since_last_write()
                        .map(|s| format!("{:>6.1}", s))
                        .unwrap_or_else(|| "  ????".into());
                    let held = match fs.process_holds_file {
                        Some(true)  => " yes",
                        Some(false) => "  no",
                        None        => "  ---",
                    };
                    let last_msg = log.tail.last()
                        .map(|l| l.message.chars().take(45).collect::<String>())
                        .unwrap_or_default();

                    let prev = seen.get(id);
                    let changed = prev.map(|e| &e.state != &state).unwrap_or(true);

                    if changed {
                        let duration = prev.map(|e| {
                            let secs = e.entered_at.elapsed().as_secs_f64();
                            format!(" [{:.1}s in {:?}]", secs, e.state)
                        }).unwrap_or_default();

                        let arrow = if prev.is_some() { "→" } else { " " };
                        println!(
                            "{:<8}  {:<40}  {}  {:<8}  {}  {:?}{}",
                            tick, id.0, secs_idle, held, arrow, state, duration
                        );
                        println!("          └─ last log: {}", last_msg);

                        seen.insert(id.clone(), TraceEntry {
                            state,
                            entered_at: Instant::now(),
                            tick,
                        });
                    }
                }
                Err(e) => eprintln!("  [{}] error: {}", id, e),
            }
        }

        tick += 1;
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

async fn resolve_ids(adapter: &ClaudeCodeAdapter, filter: Option<&str>) -> Result<Vec<SessionId>> {
    let all = adapter.discover().await?;
    if all.is_empty() {
        eprintln!("No sessions found.");
        return Ok(vec![]);
    }
    Ok(match filter {
        Some(s) => all.into_iter().filter(|id| id.0 == s).collect(),
        None => all,
    })
}

fn fmt_state(s: &SessionState) -> String {
    match s {
        SessionState::Running => "Running".into(),
        SessionState::AwaitingInput { reason: Some(r) } => format!("Awaiting({r})"),
        SessionState::AwaitingInput { reason: None } => "Awaiting".into(),
        SessionState::Idle => "Idle".into(),
        SessionState::Done => "Done".into(),
        SessionState::Error { message: m } => format!("Error({m})"),
        SessionState::Unknown => "Unknown".into(),
    }
}
