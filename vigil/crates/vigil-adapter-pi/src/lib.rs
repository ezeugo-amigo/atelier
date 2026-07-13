//! Pi adapter for Vigil.
//!
//! Session files: ~/.pi/agent/sessions/<encoded-path>/<ISO8601>_<uuid>.jsonl
//! Path encoding: /Users/foo/bar → --Users-foo-bar--
//!
//! State is derived from the last message in the most recent session JSONL
//! combined with how recently the file was written (mtime proxy for activity).

pub mod classifier;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use vigil_core::{AgentAdapter, AgentKind, LogEvent, ProbeResult, SessionId, ToolKind, VigilError};

pub struct PiAdapter;

#[async_trait::async_trait]
impl AgentAdapter for PiAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Pi
    }

    async fn probe(&self, dir: &Path) -> ProbeResult {
        let session_dir = sessions_dir().join(encode_path(dir));
        let Some(latest) = find_latest(&session_dir) else {
            return ProbeResult::no_session();
        };

        let session_id = session_id_from_path(&latest);

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
        let last_msg = classifier::parse_last_message(&content);
        let state = classifier::classify(last_msg, secs_since_write);

        // Extract last user message for display
        let last_user_message = last_user_message(&content);

        ProbeResult {
            state,
            session_id,
            last_activity,
            last_user_message,
        }
    }

    async fn recent_log(&self, dir: &Path) -> Vec<String> {
        let session_dir = sessions_dir().join(encode_path(dir));
        let Some(latest) = find_latest(&session_dir) else {
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
                format_message(&val)
            })
            .collect()
    }

    async fn recent_log_events(&self, dir: &Path) -> Vec<LogEvent> {
        let session_dir = sessions_dir().join(encode_path(dir));
        let Some(latest) = find_latest(&session_dir) else {
            return vec![];
        };
        let Ok(content) = tokio::fs::read_to_string(&latest).await else {
            return vec![];
        };
        // Parse only the last 500 lines — enough for ~50 turns, avoids reading full large sessions.
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
        let mut cmd = std::process::Command::new("pi");
        cmd.arg("--print")
            .arg(msg)
            .current_dir(dir);
        let mut cmd = vigil_core::wrap_agent_harness_command(AgentKind::Pi, dir, cmd);
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        tokio::process::Command::from(cmd)
            .spawn()
            .map_err(|e| VigilError::ProcessProbe(format!("pi spawn failed: {e}")))?;
        Ok(())
    }

    async fn send_message(
        &self,
        dir: &Path,
        _session_id: &SessionId,
        msg: &str,
    ) -> Result<(), VigilError> {
        let session_dir = sessions_dir().join(encode_path(dir));
        let Some(latest) = find_latest(&session_dir) else {
            return Err(VigilError::NotSupported("no Pi session file found".into()));
        };

        // Use `pi --session <file> --print <msg>` — fire and forget.
        let mut cmd = std::process::Command::new("pi");
        cmd.arg("--session")
            .arg(&latest)
            .arg("--print")
            .arg(msg)
            .current_dir(dir);
        let mut cmd = vigil_core::wrap_agent_harness_command(AgentKind::Pi, dir, cmd);
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        tokio::process::Command::from(cmd)
            .spawn()
            .map_err(|e| VigilError::ProcessProbe(format!("pi spawn failed: {e}")))?;
        Ok(())
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

// ── Path / session helpers ────────────────────────────────────────────────────

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

/// Return the most recent JSONL file in a session directory (alphabetical = chronological).
fn find_latest(session_dir: &Path) -> Option<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(session_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |e| e == "jsonl"))
        .collect();
    files.sort();
    files.into_iter().last()
}

/// Extract the UUID from `{ISO8601}_{uuid}.jsonl`.
fn session_id_from_path(path: &Path) -> Option<SessionId> {
    let stem = path.file_stem()?.to_str()?;
    let uuid = stem.splitn(2, '_').nth(1)?;
    Some(SessionId(uuid.to_string()))
}

/// Find the last user message text in the session for display.
fn last_user_message(content: &str) -> Option<String> {
    content.lines().rev().find_map(|line| {
        let val: serde_json::Value = serde_json::from_str(line).ok()?;
        if val["message"]["role"].as_str() != Some("user") {
            return None;
        }
        val["message"]["content"]
            .as_array()?
            .iter()
            .find(|c| c["type"] == "text")
            .and_then(|c| c["text"].as_str())
            .map(|s| s.chars().take(120).collect())
    })
}

// ── Structured conversation events (for timeline log-view) ───────────────────

/// Parse a Pi session JSONL into structured `LogEvent`s grouped by turn.
///
/// A turn is: UserMessage → zero or more ToolGroup entries → AgentMessage.
/// In-progress turns (user sent, agent still working) are flushed as partial.
fn parse_conversation_events(content: &str) -> Vec<LogEvent> {
    use std::collections::HashMap;

    let mut events: Vec<LogEvent> = Vec::new();
    // Pending state for the current turn being built.
    let mut pending_user: Option<(String, Option<String>)> = None;
    let mut pending_tools: HashMap<String, u32> = HashMap::new();

    for line in content.lines() {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let time = val["timestamp"]
            .as_str()
            .and_then(|ts| ts.get(11..19))
            .map(str::to_string);
        let msg = &val["message"];

        match msg["role"].as_str() {
            Some("user") => {
                // Start of a new turn: flush any previous incomplete turn.
                flush_pending_turn(&mut events, &mut pending_user, &mut pending_tools);
                let text: String = msg["content"]
                    .as_array()
                    .and_then(|a| a.iter().find(|c| c["type"] == "text"))
                    .and_then(|c| c["text"].as_str())
                    .unwrap_or("")
                    .chars()
                    .take(2000)
                    .collect();
                pending_user = Some((text, time));
            }
            Some("assistant") => {
                let content_arr = msg["content"]
                    .as_array()
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]);
                let last_type = content_arr.last().and_then(|c| c["type"].as_str());

                if last_type == Some("toolCall") {
                    // Accumulate all toolCall items in this message.
                    for item in content_arr {
                        if item["type"] == "toolCall" {
                            let name = item["name"].as_str().unwrap_or("?").to_string();
                            *pending_tools.entry(name).or_insert(0) += 1;
                        }
                    }
                } else {
                    // Final text response — emit the completed turn.
                    let text: String = content_arr
                        .iter()
                        .find(|c| c["type"] == "text")
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
                    emit_tool_group(&mut events, &mut pending_tools);
                    events.push(LogEvent::AgentMessage {
                        text,
                        time,
                        label: "Pi".to_string(),
                    });
                }
            }
            // toolResult messages are metadata; the tool name is already captured from toolCall.
            _ => {}
        }
    }

    // Flush any in-progress turn at end of file.
    flush_pending_turn(&mut events, &mut pending_user, &mut pending_tools);

    events
}

fn flush_pending_turn(
    events: &mut Vec<LogEvent>,
    pending_user: &mut Option<(String, Option<String>)>,
    pending_tools: &mut std::collections::HashMap<String, u32>,
) {
    if let Some((text, time)) = pending_user.take() {
        events.push(LogEvent::UserMessage { text, time });
    }
    emit_tool_group(events, pending_tools);
}

fn emit_tool_group(
    events: &mut Vec<LogEvent>,
    pending_tools: &mut std::collections::HashMap<String, u32>,
) {
    if pending_tools.is_empty() {
        return;
    }
    // Merge by ToolKind so that e.g. "read" and "readfile" collapse together.
    let mut kind_map: std::collections::HashMap<ToolKind, u32> = std::collections::HashMap::new();
    for (name, count) in pending_tools.drain() {
        let kind = ToolKind::from_name(&name);
        *kind_map.entry(kind).or_insert(0) += count;
    }
    // Stable order: Read, Bash, Edit, Other.
    let mut tools: Vec<(ToolKind, u32)> = kind_map.into_iter().collect();
    tools.sort_by_key(|(k, _)| match k {
        ToolKind::Read => 0,
        ToolKind::Bash => 1,
        ToolKind::Edit => 2,
        ToolKind::Other(_) => 3,
    });
    events.push(LogEvent::ToolGroup { tools });
}

// ── Message formatting (for log overlay) ─────────────────────────────────────

fn format_message(val: &serde_json::Value) -> Option<String> {
    if val["type"].as_str()? != "message" {
        return None;
    }
    let msg = &val["message"];
    match msg["role"].as_str()? {
        "user" => {
            let text = msg["content"]
                .as_array()?
                .iter()
                .find(|c| c["type"] == "text")?["text"]
                .as_str()?;
            Some(format!(
                "YOU   {}",
                text.chars().take(100).collect::<String>()
            ))
        }
        "assistant" => {
            let content = msg["content"].as_array()?;
            for item in content {
                match item["type"].as_str() {
                    Some("text") => {
                        let text = item["text"].as_str().unwrap_or("");
                        return Some(format!(
                            "PI    {}",
                            text.chars().take(100).collect::<String>()
                        ));
                    }
                    Some("toolCall") => {
                        let name = item["name"].as_str().unwrap_or("?");
                        let arg_hint = item["arguments"]
                            .as_object()
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
            let snippet = msg["content"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|c| c["text"].as_str())
                .map(|s| s.chars().take(80).collect::<String>())
                .unwrap_or_default();
            Some(format!("RSLT  {name} → {snippet}"))
        }
        _ => None,
    }
}
