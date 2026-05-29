//! On-demand "recap" of a session's recent activity.
//!
//! Vigil never holds an API key — like the rest of the app, the recap shells
//! out to the already-authenticated `claude` CLI in `--print` mode, feeding it
//! the last ~20 messages of the session and asking for a short summary of what
//! has been worked on. The result is pinned to the corner of the log view to
//! ease context-switching between sessions.

use std::process::Stdio;
use std::time::Duration;

use vigil_core::LogEvent;

/// How many recent message-bearing events to feed the summarizer.
const RECAP_WINDOW: usize = 20;
/// Model alias for the one-shot summary — haiku keeps it fast and cheap.
const RECAP_MODEL: &str = "haiku";
/// Give up on the summary if `claude` hasn't returned within this long.
const RECAP_TIMEOUT: Duration = Duration::from_secs(45);
/// Cap each message so one long paste can't dominate the prompt.
const PER_MESSAGE_CHARS: usize = 800;
/// Raw-log fallback (Claude debug logs): how many trailing lines to include.
const RAW_LOG_LINES: usize = 60;

/// State of the recap for a single session, cached per container id.
#[derive(Debug, Clone, Default)]
pub enum Recap {
    /// Not yet requested.
    #[default]
    Idle,
    /// A summarization call is in flight.
    Loading,
    /// A summary was produced.
    Ready(String),
    /// The call failed; holds a short reason for display.
    Error(String),
}

/// Build the transcript fed to the summarizer from structured events.
fn transcript_from_events(events: &[LogEvent]) -> String {
    // Keep only the message-bearing events, then take the last RECAP_WINDOW.
    let msgs: Vec<&LogEvent> = events
        .iter()
        .filter(|e| matches!(e, LogEvent::UserMessage { .. } | LogEvent::AgentMessage { .. }))
        .collect();
    let start = msgs.len().saturating_sub(RECAP_WINDOW);

    let mut out = String::new();
    for event in &msgs[start..] {
        match event {
            LogEvent::UserMessage { text, .. } => {
                out.push_str("USER: ");
                out.push_str(&truncate(text, PER_MESSAGE_CHARS));
                out.push_str("\n\n");
            }
            LogEvent::AgentMessage { text, label, .. } => {
                out.push_str(&label.to_uppercase());
                out.push_str(": ");
                out.push_str(&truncate(text, PER_MESSAGE_CHARS));
                out.push_str("\n\n");
            }
            LogEvent::ToolGroup { .. } => {}
        }
    }
    out
}

/// Build the transcript from raw debug-log lines (Claude Code fallback).
fn transcript_from_lines(lines: &[String]) -> String {
    let start = lines.len().saturating_sub(RAW_LOG_LINES);
    lines[start..].join("\n")
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{truncated}…")
    }
}

const PROMPT_PREAMBLE: &str = "You are summarizing a coding-agent session for a developer who is \
context-switching between several sessions. In 2-3 short sentences, plainly state what has been \
worked on so far — name concrete files, features, or problems where possible. No preamble, no \
bullet points, no markdown headers. Here is the recent transcript:\n\n";

/// Generate a recap by shelling out to `claude --print`. Returns the summary
/// text on success, or a short human-readable error string on failure.
pub async fn generate(events: &[LogEvent], lines: &[String]) -> Result<String, String> {
    let transcript = if !events.is_empty() {
        transcript_from_events(events)
    } else {
        transcript_from_lines(lines)
    };

    if transcript.trim().is_empty() {
        return Err("nothing to summarize yet".to_string());
    }

    let prompt = format!("{PROMPT_PREAMBLE}{transcript}");

    // Run in a neutral directory so the one-shot call doesn't pick up a
    // project's CLAUDE.md, hooks, or MCP servers — we want a fast, clean
    // summary, not a project-aware agent turn.
    let call = tokio::process::Command::new("claude")
        .arg("--print")
        .arg("--model")
        .arg(RECAP_MODEL)
        .arg(&prompt)
        .env_remove("CLAUDECODE")
        .current_dir(std::env::temp_dir())
        // Reap the child if we hit the timeout and drop the future.
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output = match tokio::time::timeout(RECAP_TIMEOUT, call).await {
        Ok(result) => result.map_err(|e| format!("claude spawn failed: {e}"))?,
        Err(_) => return Err("timed out after 45s".to_string()),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = stderr.trim().lines().next().unwrap_or("claude exited with error");
        return Err(msg.chars().take(120).collect());
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return Err("empty summary".to_string());
    }
    Ok(text)
}
