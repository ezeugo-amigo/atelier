//! Codex adapter for Vigil.
//!
//! Session files: ~/.codex/sessions/<YYYY>/<MM>/<DD>/<name>-<timestamp>-<uuid>.jsonl
//! Sessions are not path-encoded into subdirectories; we scan the last 30 days and
//! filter by the `cwd` field in each file's first `session_meta` line.

pub mod classifier;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use vigil_core::{AgentAdapter, AgentKind, LogEvent, ProbeResult, SessionId, ToolKind, VigilError};

pub struct CodexAdapter;

/// Locate the codex binary. Prefers the Conductor-bundled binary (which is the
/// version users actually have installed) over the broken npm wrapper in PATH.
fn codex_bin() -> std::ffi::OsString {
    let conductor = directories::BaseDirs::new()
        .map(|b| {
            b.home_dir()
                .join("Library/Application Support/com.conductor.app/bin/codex")
        })
        .filter(|p| p.exists());

    if let Some(path) = conductor {
        path.into_os_string()
    } else {
        "codex".into()
    }
}

#[async_trait::async_trait]
impl AgentAdapter for CodexAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }

    async fn probe(&self, dir: &Path) -> ProbeResult {
        let Some(latest) = find_latest_for_dir(dir) else {
            return ProbeResult::no_session();
        };

        let secs_since_write = latest
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(f64::MAX);

        let last_activity = latest
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0).unwrap_or_default());

        let content = tokio::fs::read_to_string(&latest).await.unwrap_or_default();
        let session_id = session_id_from_content(&content);
        let last_msg = classifier::parse_last_message(&content);
        let state = classifier::classify(last_msg, secs_since_write);
        let last_user_message = last_user_message(&content);

        ProbeResult {
            state,
            session_id,
            last_activity,
            last_user_message,
        }
    }

    async fn recent_log(&self, dir: &Path) -> Vec<String> {
        let Some(latest) = find_latest_for_dir(dir) else {
            return vec![];
        };
        let Ok(content) = tokio::fs::read_to_string(&latest).await else {
            return vec![];
        };

        let lines: Vec<&str> = content.lines().collect();
        let tail = if lines.len() > 80 {
            &lines[lines.len() - 80..]
        } else {
            &lines
        };

        tail.iter()
            .filter_map(|line| {
                let val: serde_json::Value = serde_json::from_str(line).ok()?;
                format_log_line(&val)
            })
            .collect()
    }

    async fn recent_log_events(&self, dir: &Path) -> Vec<LogEvent> {
        let Some(latest) = find_latest_for_dir(dir) else {
            return vec![];
        };
        let Ok(content) = tokio::fs::read_to_string(&latest).await else {
            return vec![];
        };
        let tail: String = content
            .lines()
            .rev()
            .take(500)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        parse_conversation_events(&tail)
    }

    async fn start_with_message(&self, dir: &Path, msg: &str) -> Result<(), VigilError> {
        tokio::process::Command::new(codex_bin())
            .arg("exec")
            .arg(msg)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| VigilError::ProcessProbe(format!("codex spawn failed: {e}")))?;
        Ok(())
    }

    async fn send_message(
        &self,
        dir: &Path,
        session_id: &SessionId,
        msg: &str,
    ) -> Result<(), VigilError> {
        tokio::process::Command::new(codex_bin())
            .arg("exec")
            .arg("resume")
            .arg(&session_id.0)
            .arg(msg)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| VigilError::ProcessProbe(format!("codex spawn failed: {e}")))?;
        Ok(())
    }

    fn attach_command(&self, session_id: &SessionId, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new(codex_bin());
        cmd.arg("resume").arg(&session_id.0).current_dir(dir);
        cmd
    }

    fn launch_command(&self, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new(codex_bin());
        cmd.current_dir(dir);
        cmd
    }
}

// ── Session discovery ─────────────────────────────────────────────────────────

fn sessions_root() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".codex").join("sessions"))
        .unwrap_or_else(|| PathBuf::from("~/.codex/sessions"))
}

/// Walk the last 30 days of session directories and return the most recently
/// modified JSONL whose `session_meta.cwd` matches `dir`.
fn find_latest_for_dir(dir: &Path) -> Option<PathBuf> {
    let root = sessions_root();
    let dir_str = dir.display().to_string();

    let cutoff = SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(30 * 24 * 3600))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let mut candidates: Vec<PathBuf> = Vec::new();

    let Ok(years) = std::fs::read_dir(&root) else {
        return None;
    };
    for year_entry in years.flatten() {
        let year_path = year_entry.path();
        let Ok(months) = std::fs::read_dir(&year_path) else {
            continue;
        };
        for month_entry in months.flatten() {
            let month_path = month_entry.path();
            let Ok(days) = std::fs::read_dir(&month_path) else {
                continue;
            };
            for day_entry in days.flatten() {
                let day_path = day_entry.path();
                // Skip entire day directories older than the cutoff.
                if let Ok(meta) = day_path.metadata() {
                    if meta.modified().map(|t| t < cutoff).unwrap_or(false) {
                        continue;
                    }
                }
                let Ok(files) = std::fs::read_dir(&day_path) else {
                    continue;
                };
                for file_entry in files.flatten() {
                    let p = file_entry.path();
                    if p.extension().map_or(true, |e| e != "jsonl") {
                        continue;
                    }
                    if cwd_matches(&p, &dir_str) {
                        candidates.push(p);
                    }
                }
            }
        }
    }

    candidates.into_iter().max_by_key(|p| {
        p.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH)
    })
}

/// Read only the first line of a session file and check if its `cwd` matches.
fn cwd_matches(path: &Path, dir: &str) -> bool {
    use std::io::{BufRead, BufReader};
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut reader = BufReader::new(file);
    let mut first_line = String::new();
    if reader.read_line(&mut first_line).is_err() {
        return false;
    }
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&first_line) else {
        return false;
    };
    val["payload"]["cwd"].as_str() == Some(dir)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract session ID from the `session_meta` first line.
fn session_id_from_content(content: &str) -> Option<SessionId> {
    let first = content.lines().next()?;
    let val: serde_json::Value = serde_json::from_str(first).ok()?;
    val["payload"]["id"]
        .as_str()
        .map(|s| SessionId(s.to_string()))
}

/// Return the last user message text (up to 120 chars) for display.
fn last_user_message(content: &str) -> Option<String> {
    content.lines().rev().find_map(|line| {
        let val: serde_json::Value = serde_json::from_str(line).ok()?;
        if val["type"].as_str() != Some("response_item") {
            return None;
        }
        if val["payload"]["role"].as_str() != Some("user") {
            return None;
        }
        val["payload"]["content"]
            .as_array()?
            .iter()
            .find(|c| c["type"] == "input_text")
            .and_then(|c| c["text"].as_str())
            .map(|s| s.chars().take(120).collect())
    })
}

// ── Log formatting ────────────────────────────────────────────────────────────

fn format_log_line(val: &serde_json::Value) -> Option<String> {
    if val["type"].as_str() != Some("response_item") {
        return None;
    }
    let payload = &val["payload"];
    match payload["role"].as_str()? {
        "user" => {
            let text = payload["content"]
                .as_array()?
                .iter()
                .find(|c| c["type"] == "input_text")?["text"]
                .as_str()?;
            Some(format!("YOU   {}", text.chars().take(100).collect::<String>()))
        }
        "assistant" => {
            let text = payload["content"]
                .as_array()?
                .iter()
                .find(|c| c["type"] == "output_text")?["text"]
                .as_str()?;
            Some(format!(
                "CODEX {}",
                text.chars().take(100).collect::<String>()
            ))
        }
        _ => None,
    }
}

// ── Structured conversation events ────────────────────────────────────────────

fn parse_conversation_events(content: &str) -> Vec<LogEvent> {
    let mut events: Vec<LogEvent> = Vec::new();
    let mut pending_user: Option<(String, Option<String>)> = None;
    // Codex tool calls aren't surfaced as distinct content items in the observed JSONL,
    // so we count non-text assistant turns as a single Bash tool group.
    let mut pending_tool_count: u32 = 0;

    for line in content.lines() {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if val["type"].as_str() != Some("response_item") {
            continue;
        }
        let payload = &val["payload"];
        let time = val["timestamp"]
            .as_str()
            .and_then(|ts| ts.get(11..19))
            .map(str::to_string);

        match payload["role"].as_str() {
            Some("user") => {
                flush_pending(&mut events, &mut pending_user, &mut pending_tool_count);
                let text: String = payload["content"]
                    .as_array()
                    .and_then(|a| a.iter().find(|c| c["type"] == "input_text"))
                    .and_then(|c| c["text"].as_str())
                    .unwrap_or("")
                    .chars()
                    .take(2000)
                    .collect();
                pending_user = Some((text, time));
            }
            Some("assistant") => {
                let content_arr = payload["content"].as_array().map(|v| v.as_slice()).unwrap_or(&[]);
                let has_text = content_arr.iter().any(|c| c["type"] == "output_text");
                if has_text {
                    let text: String = content_arr
                        .iter()
                        .find(|c| c["type"] == "output_text")
                        .and_then(|c| c["text"].as_str())
                        .unwrap_or("")
                        .chars()
                        .take(2000)
                        .collect();
                    if let Some((user_text, user_time)) = pending_user.take() {
                        events.push(LogEvent::UserMessage {
                            text: user_text,
                            time: user_time,
                        });
                    }
                    if pending_tool_count > 0 {
                        events.push(LogEvent::ToolGroup {
                            tools: vec![(ToolKind::Bash, pending_tool_count)],
                        });
                        pending_tool_count = 0;
                    }
                    events.push(LogEvent::AgentMessage {
                        text,
                        time,
                        label: "Codex".to_string(),
                    });
                } else {
                    // Non-text assistant turn (tool call round-trip).
                    pending_tool_count += 1;
                }
            }
            _ => {}
        }
    }

    flush_pending(&mut events, &mut pending_user, &mut pending_tool_count);
    events
}

fn flush_pending(
    events: &mut Vec<LogEvent>,
    pending_user: &mut Option<(String, Option<String>)>,
    pending_tool_count: &mut u32,
) {
    if let Some((text, time)) = pending_user.take() {
        events.push(LogEvent::UserMessage { text, time });
    }
    if *pending_tool_count > 0 {
        events.push(LogEvent::ToolGroup {
            tools: vec![(ToolKind::Bash, *pending_tool_count)],
        });
        *pending_tool_count = 0;
    }
}
