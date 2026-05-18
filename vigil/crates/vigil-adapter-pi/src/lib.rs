//! Pi adapter for Vigil.
//!
//! Session files: ~/.pi/agent/sessions/<encoded-path>/<ISO8601>-<uuid>.jsonl
//! Path encoding: /Users/foo/bar → --Users-foo-bar--
//!
//! State classification is process-based (lsof +D).
//! Log view parses the most recent JSONL session file.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use vigil_core::{AgentAdapter, AgentKind, ProbeResult, SessionId, SessionState};

pub struct PiAdapter;

#[async_trait::async_trait]
impl AgentAdapter for PiAdapter {
    fn kind(&self) -> AgentKind { AgentKind::Pi }

    async fn probe(&self, dir: &Path) -> ProbeResult {
        let state = classify(dir).await;
        let session_id = latest_session_id(dir);
        ProbeResult {
            state,
            session_id,
            last_activity: newest_mtime(dir)
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| {
                    chrono::DateTime::from_timestamp(d.as_secs() as i64, 0).unwrap_or_default()
                }),
            last_user_message: None,
        }
    }

    async fn recent_log(&self, dir: &Path) -> Vec<String> {
        let session_dir = sessions_dir().join(encode_path(dir));
        let Ok(read_dir) = std::fs::read_dir(&session_dir) else { return vec![]; };

        // Files are named {ISO8601}-{uuid}.jsonl; alphabetical sort = chronological
        let mut files: Vec<PathBuf> = read_dir
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map_or(false, |e| e == "jsonl"))
            .collect();
        files.sort();

        let Some(latest) = files.last() else { return vec![]; };
        let Ok(content) = tokio::fs::read_to_string(latest).await else { return vec![]; };

        let lines: Vec<&str> = content.lines().collect();
        let tail = if lines.len() > 80 { &lines[lines.len() - 80..] } else { &lines };

        tail.iter()
            .filter_map(|line| {
                let val: serde_json::Value = serde_json::from_str(line).ok()?;
                format_message(&val)
            })
            .collect()
    }

    fn attach_command(&self, session_id: &SessionId, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new("pi");
        cmd.arg("--session").arg(&session_id.0);
        cmd.current_dir(dir);
        cmd
    }

    fn launch_command(&self, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new("pi");
        cmd.current_dir(dir);
        cmd
    }
}

// ── Path encoding ────────────────────────────────────────────────────────────

/// `/Users/foo/bar` → `--Users-foo-bar--`
fn encode_path(dir: &Path) -> String {
    let s = dir.display().to_string();
    format!("--{}--", s.trim_start_matches('/').replace('/', "-"))
}

fn sessions_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".pi").join("agent").join("sessions"))
        .unwrap_or_else(|| PathBuf::from("~/.pi/agent/sessions"))
}

// ── Message formatting ────────────────────────────────────────────────────────

fn format_message(val: &serde_json::Value) -> Option<String> {
    if val["type"].as_str()? != "message" {
        return None;
    }
    let msg = &val["message"];
    match msg["role"].as_str()? {
        "user" => {
            let text = msg["content"].as_array()?
                .iter()
                .find(|c| c["type"] == "text")?["text"]
                .as_str()?;
            let snippet: String = text.chars().take(100).collect();
            Some(format!("YOU   {snippet}"))
        }
        "assistant" => {
            let content = msg["content"].as_array()?;
            // Prefer text over toolCall; skip thinking
            for item in content {
                match item["type"].as_str() {
                    Some("text") => {
                        let text = item["text"].as_str().unwrap_or("");
                        let snippet: String = text.chars().take(100).collect();
                        return Some(format!("PI    {snippet}"));
                    }
                    Some("toolCall") => {
                        let name = item["name"].as_str().unwrap_or("?");
                        let args = &item["arguments"];
                        // Show first meaningful arg value
                        let arg_hint = args.as_object()
                            .and_then(|o| o.values().next())
                            .and_then(|v| v.as_str())
                            .map(|s| s.chars().take(50).collect::<String>())
                            .unwrap_or_default();
                        return Some(format!("TOOL  {name}({arg_hint})"));
                    }
                    _ => {}
                }
            }
            None
        }
        "toolResult" => {
            let name = msg["toolName"].as_str().unwrap_or("?");
            let snippet = msg["content"].as_array()
                .and_then(|a| a.first())
                .and_then(|c| c["text"].as_str())
                .map(|s| s.chars().take(80).collect::<String>())
                .unwrap_or_default();
            Some(format!("RSLT  {name} → {snippet}"))
        }
        _ => None,
    }
}

// ── Session ID ────────────────────────────────────────────────────────────────

/// Find the most recent JSONL session file for `dir` and extract its UUID.
/// Files are named `{ISO8601}_{uuid}.jsonl`; the UUID follows the first `_`.
fn latest_session_id(dir: &Path) -> Option<SessionId> {
    let session_dir = sessions_dir().join(encode_path(dir));
    let mut files: Vec<PathBuf> = std::fs::read_dir(&session_dir).ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |e| e == "jsonl"))
        .collect();
    files.sort();
    let latest = files.last()?;
    let stem = latest.file_stem()?.to_str()?;
    let uuid = stem.splitn(2, '_').nth(1)?;
    Some(SessionId(uuid.to_string()))
}

// ── State classification ──────────────────────────────────────────────────────

async fn classify(dir: &Path) -> SessionState {
    match pi_is_active(dir).await {
        Some(true) => SessionState::Running,
        Some(false) => {
            let secs_idle = newest_mtime(dir)
                .and_then(|t| t.elapsed().ok())
                .map(|d| d.as_secs_f64())
                .unwrap_or(f64::MAX);
            if secs_idle < 30.0 { SessionState::Idle } else { SessionState::Done }
        }
        None => SessionState::NoSession,
    }
}

async fn pi_is_active(dir: &Path) -> Option<bool> {
    let output = tokio::process::Command::new("lsof")
        .args(["+D"])
        .arg(dir)
        .output()
        .await
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let active = stdout.lines().any(|l| {
        let lower = l.to_ascii_lowercase();
        lower.starts_with("pi ") || lower.contains("/pi ")
    });
    tracing::debug!("pi_is_active({}) = {active}", dir.display());
    Some(active)
}

fn newest_mtime(dir: &Path) -> Option<SystemTime> {
    std::fs::read_dir(dir).ok()?.flatten()
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
}
