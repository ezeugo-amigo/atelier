//! OpenCode adapter for Vigil.
//!
//! OpenCode ≥ 1.17 stores all state in a single SQLite database:
//!   ~/.local/share/opencode/opencode.db
//!
//! Key tables:
//!   session  – id, directory (CWD), time_updated (ms)
//!   message  – id, session_id, time_created (ms), data (JSON)
//!   part     – id, message_id, session_id, time_created (ms), data (JSON)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use vigil_core::{AgentAdapter, AgentKind, LogEvent, ProbeResult, SessionId, SessionState, ToolKind, VigilError};

pub struct OpenCodeAdapter;

fn opencode_bin() -> std::ffi::OsString {
    if std::env::var_os("PATH")
        .unwrap_or_default()
        .to_string_lossy()
        .split(':')
        .map(std::path::Path::new)
        .any(|dir| dir.join("opencode").exists())
    {
        return "opencode".into();
    }
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".opencode").join("bin").join("opencode"))
        .filter(|p| p.exists())
        .map(|p| p.into_os_string())
        .unwrap_or_else(|| "opencode".into())
}

/// OpenCode uses XDG_DATA_HOME on all platforms (including macOS).
fn db_path() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            directories::BaseDirs::new()
                .map(|b| b.home_dir().join(".local").join("share"))
                .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        })
        .join("opencode")
        .join("opencode.db")
}

#[async_trait::async_trait]
impl AgentAdapter for OpenCodeAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::OpenCode
    }

    async fn probe(&self, dir: &Path) -> ProbeResult {
        let dir = dir.to_path_buf();
        tokio::task::spawn_blocking(move || probe_sync(&dir))
            .await
            .unwrap_or_else(|_| ProbeResult::no_session())
    }

    async fn recent_log(&self, dir: &Path) -> Vec<String> {
        let dir = dir.to_path_buf();
        tokio::task::spawn_blocking(move || recent_log_sync(&dir))
            .await
            .unwrap_or_default()
    }

    async fn recent_log_events(&self, dir: &Path) -> Vec<LogEvent> {
        let dir = dir.to_path_buf();
        tokio::task::spawn_blocking(move || recent_log_events_sync(&dir))
            .await
            .unwrap_or_default()
    }

    async fn send_message(
        &self,
        dir: &Path,
        session_id: &SessionId,
        msg: &str,
    ) -> Result<(), VigilError> {
        let mut cmd = std::process::Command::new(opencode_bin());
        cmd.arg("run")
            .arg("--session")
            .arg(&session_id.0)
            .arg(msg)
            .current_dir(dir);
        let mut cmd = vigil_core::wrap_agent_harness_command(AgentKind::OpenCode, dir, cmd);
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        tokio::process::Command::from(cmd)
            .spawn()
            .map_err(|e| VigilError::ProcessProbe(format!("opencode spawn failed: {e}")))?;
        Ok(())
    }

    async fn start_with_message(&self, dir: &Path, msg: &str) -> Result<(), VigilError> {
        let mut cmd = std::process::Command::new(opencode_bin());
        cmd.arg("run")
            .arg(msg)
            .current_dir(dir);
        let mut cmd = vigil_core::wrap_agent_harness_command(AgentKind::OpenCode, dir, cmd);
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        tokio::process::Command::from(cmd)
            .spawn()
            .map_err(|e| VigilError::ProcessProbe(format!("opencode spawn failed: {e}")))?;
        Ok(())
    }

    fn attach_command(&self, session_id: &SessionId, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new(opencode_bin());
        cmd.arg("--session").arg(&session_id.0).current_dir(dir);
        cmd
    }

    fn launch_command(&self, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new(opencode_bin());
        cmd.current_dir(dir);
        cmd
    }
}

// ── Blocking helpers (run via spawn_blocking) ─────────────────────────────────

fn open_db() -> Option<rusqlite::Connection> {
    let path = db_path();
    if !path.exists() {
        return None;
    }
    // Open read-only to avoid write conflicts with the running opencode process.
    rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()
}

struct SessionRow {
    id: String,
    time_updated: i64, // ms
}

fn latest_session_for_dir(conn: &rusqlite::Connection, dir: &str) -> Option<SessionRow> {
    conn.query_row(
        "SELECT id, time_updated FROM session \
         WHERE directory = ?1 AND time_archived IS NULL \
         ORDER BY time_updated DESC LIMIT 1",
        rusqlite::params![dir],
        |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                time_updated: row.get(1)?,
            })
        },
    )
    .ok()
}

struct MessageRow {
    id: String,
    time_created: i64,
    data: serde_json::Value,
}

fn messages_for_session(conn: &rusqlite::Connection, session_id: &str) -> Vec<MessageRow> {
    let mut stmt = match conn.prepare(
        "SELECT id, time_created, data FROM message \
         WHERE session_id = ?1 ORDER BY time_created ASC, id ASC",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map(rusqlite::params![session_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
    })
    .ok()
    .into_iter()
    .flatten()
    .filter_map(|r| r.ok())
    .filter_map(|(id, tc, data_str)| {
        serde_json::from_str(&data_str)
            .ok()
            .map(|data| MessageRow { id, time_created: tc, data })
    })
    .collect()
}

fn parts_for_message(conn: &rusqlite::Connection, message_id: &str) -> Vec<serde_json::Value> {
    let mut stmt = match conn.prepare(
        "SELECT data FROM part WHERE message_id = ?1 ORDER BY id ASC",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map(rusqlite::params![message_id], |row| row.get::<_, String>(0))
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|r| r.ok())
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect()
}

// ── Probe ─────────────────────────────────────────────────────────────────────

fn probe_sync(dir: &Path) -> ProbeResult {
    let Some(conn) = open_db() else {
        return ProbeResult::no_session();
    };
    let dir_str = dir.display().to_string();
    let Some(session) = latest_session_for_dir(&conn, &dir_str) else {
        return ProbeResult::no_session();
    };

    let last_activity = chrono::DateTime::from_timestamp_millis(session.time_updated);
    let messages = messages_for_session(&conn, &session.id);
    let (state, last_user_message) = classify(&conn, &messages, session.time_updated);

    ProbeResult {
        state,
        session_id: Some(SessionId(session.id)),
        last_activity,
        last_user_message,
    }
}

fn classify(
    conn: &rusqlite::Connection,
    messages: &[MessageRow],
    session_updated_ms: i64,
) -> (SessionState, Option<String>) {
    let now_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let state = match messages.last() {
        None => SessionState::Unknown,
        Some(msg) => {
            let role = msg.data["role"].as_str().unwrap_or("");
            let has_error = msg.data.get("error").map_or(false, |e| !e.is_null());
            let completed = msg.data["time"]["completed"].as_i64();

            if role == "assistant" && has_error {
                SessionState::Error { message: "API error".to_string() }
            } else if role == "assistant" && completed.is_none() {
                // Assistant message started but not yet completed — streaming.
                SessionState::Running
            } else if role == "user" {
                let age_ms = now_ms - msg.time_created;
                if age_ms < 120_000 {
                    SessionState::Running
                } else {
                    SessionState::Idle
                }
            } else {
                // Completed assistant turn — check recency.
                let age_ms = now_ms - session_updated_ms;
                if age_ms < 300_000 {
                    SessionState::AwaitingInput { reason: None }
                } else {
                    SessionState::Idle
                }
            }
        }
    };

    let last_user_message = messages
        .iter()
        .rev()
        .find(|m| m.data["role"].as_str() == Some("user"))
        .and_then(|msg| {
            // Try to find actual text in parts; fall back to summary title.
            let parts = parts_for_message(conn, &msg.id);
            parts
                .iter()
                .find(|p| p["type"].as_str() == Some("text"))
                .and_then(|p| p["text"].as_str())
                .map(|t| t.chars().take(120).collect())
                .or_else(|| {
                    msg.data["summary"]["title"]
                        .as_str()
                        .filter(|t| !t.is_empty())
                        .map(|t| t.chars().take(120).collect())
                })
        });

    (state, last_user_message)
}

// ── Log / timeline ────────────────────────────────────────────────────────────

fn recent_log_sync(dir: &Path) -> Vec<String> {
    let Some(conn) = open_db() else { return vec![] };
    let dir_str = dir.display().to_string();
    let Some(session) = latest_session_for_dir(&conn, &dir_str) else { return vec![] };
    let messages = messages_for_session(&conn, &session.id);

    messages
        .iter()
        .filter_map(|msg| {
            let role = msg.data["role"].as_str()?;
            let parts = parts_for_message(&conn, &msg.id);
            let text: String = parts
                .iter()
                .filter(|p| p["type"].as_str() == Some("text"))
                .filter_map(|p| p["text"].as_str())
                .take(1)
                .flat_map(|t| t.chars().take(100))
                .collect();
            if text.is_empty() {
                return None;
            }
            let prefix = if role == "user" { "YOU      " } else { "OPENCODE " };
            Some(format!("{}{}", prefix, text))
        })
        .collect()
}

fn recent_log_events_sync(dir: &Path) -> Vec<LogEvent> {
    let Some(conn) = open_db() else { return vec![] };
    let dir_str = dir.display().to_string();
    let Some(session) = latest_session_for_dir(&conn, &dir_str) else { return vec![] };
    let messages = messages_for_session(&conn, &session.id);
    build_log_events(&conn, &messages)
}

fn build_log_events(conn: &rusqlite::Connection, messages: &[MessageRow]) -> Vec<LogEvent> {
    let mut events: Vec<LogEvent> = Vec::new();

    for msg in messages {
        let role = match msg.data["role"].as_str() {
            Some(r) => r,
            None => continue,
        };
        let parts = parts_for_message(conn, &msg.id);

        if role == "user" {
            let text = parts
                .iter()
                .find(|p| p["type"].as_str() == Some("text"))
                .and_then(|p| p["text"].as_str())
                .map(|t| t.chars().take(2000).collect::<String>())
                .or_else(|| {
                    msg.data["summary"]["title"]
                        .as_str()
                        .filter(|t| !t.is_empty())
                        .map(|t| t.chars().take(2000).collect())
                })
                .unwrap_or_default();
            if !text.is_empty() {
                events.push(LogEvent::UserMessage { text, time: None });
            }
        } else if role == "assistant" {
            let tool_parts: Vec<&serde_json::Value> =
                parts.iter().filter(|p| p["type"].as_str() == Some("tool")).collect();
            if !tool_parts.is_empty() {
                let mut counts: HashMap<String, u32> = HashMap::new();
                for p in &tool_parts {
                    let name = p["tool"].as_str().unwrap_or("other").to_string();
                    *counts.entry(name).or_insert(0) += 1;
                }
                let tools: Vec<(ToolKind, u32)> = counts
                    .into_iter()
                    .map(|(name, count)| (classify_tool_name(&name), count))
                    .collect();
                events.push(LogEvent::ToolGroup { tools });
            }

            let text: String = parts
                .iter()
                .filter(|p| p["type"].as_str() == Some("text"))
                .filter_map(|p| p["text"].as_str())
                .flat_map(|t| t.chars().take(2000))
                .collect();
            if !text.is_empty() {
                events.push(LogEvent::AgentMessage {
                    text,
                    time: None,
                    label: "OpenCode".to_string(),
                });
            }
        }
    }

    events
}

fn classify_tool_name(name: &str) -> ToolKind {
    let lower = name.to_lowercase();
    if lower.contains("read") || lower.contains("glob") || lower.contains("grep") || lower.contains("list") {
        ToolKind::Read
    } else if lower.contains("bash") || lower.contains("shell") || lower.contains("run") || lower.contains("exec") {
        ToolKind::Bash
    } else if lower.contains("edit") || lower.contains("write") || lower.contains("patch") {
        ToolKind::Edit
    } else {
        ToolKind::Other(name.to_string())
    }
}
