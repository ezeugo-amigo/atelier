use vigil_core::{FsSignals, SessionLog, SessionState};

/// Scan last 50 lines in reverse for awaiting signals.
/// Stops if `UserPromptSubmit` is found (user already responded).
fn detect_awaiting_signal(log: &SessionLog) -> Option<String> {
    for line in log.tail.iter().rev().take(50) {
        let msg = &line.message;

        // User responded — no longer awaiting
        if msg.contains("UserPromptSubmit") {
            return None;
        }

        if msg.contains("Notification with query: permission_prompt") {
            return Some("permission prompt".to_string());
        }

        if let Some(tool) = extract_permission_suggestions(msg) {
            return Some(format!("awaiting approval: {tool}"));
        }

        if msg.contains("[onCancel]") && msg.contains("streamMode=tool-use") {
            return Some("cancelled tool-use, awaiting next message".to_string());
        }
    }
    None
}

fn extract_permission_suggestions(msg: &str) -> Option<String> {
    // "Permission suggestions for {Tool}:" or "Permission suggestions for {Tool}: ["
    let prefix = "Permission suggestions for ";
    if let Some(rest) = msg.strip_prefix(prefix) {
        // Take only up to the colon
        let tool = rest.split(':').next().unwrap_or(rest).trim().to_string();
        return Some(tool);
    }
    None
}

/// Scan last 20 lines for executePermissionRequestHooks or Permission suggestions,
/// stopping on UserPromptSubmit or "Stream started".
fn tail_ends_with_permission_context(log: &SessionLog) -> bool {
    for line in log.tail.iter().rev().take(20) {
        let msg = &line.message;
        if msg.contains("UserPromptSubmit") || msg.contains("Stream started") {
            return false;
        }
        if msg.contains("executePermissionRequestHooks called")
            || msg.contains("Permission suggestions for")
        {
            return true;
        }
    }
    false
}

/// True if the tail shows the prompt_suggestion subagent is running or finished —
/// meaning the main response is complete and only background bookkeeping remains.
/// Stops looking back if UserPromptSubmit is found (user already sent next message).
fn in_prompt_suggestion_phase(log: &SessionLog) -> bool {
    for line in log.tail.iter().rev().take(15) {
        let msg = &line.message;
        if msg.contains("UserPromptSubmit") {
            return false;
        }
        if msg.contains("Forked agent [prompt_suggestion]")
            || msg.contains("[Speculation] enabled=false")
        {
            return true;
        }
    }
    false
}

/// True if we're mid-stream: the most recent "Stream started" comes *after*
/// the most recent end-of-stream marker in the full tail.
/// Using positions avoids false positives when the prompt_suggestion subagent
/// writes many lines after "Stopped caffeinate", pushing it out of a fixed window.
fn mid_stream(log: &SessionLog) -> bool {
    let mut last_start: Option<usize> = None;
    let mut last_end: Option<usize> = None;

    for (i, line) in log.tail.iter().enumerate() {
        let msg = &line.message;
        if msg.contains("Stream started") {
            last_start = Some(i);
        }
        if msg.contains("Stopped caffeinate, allowing sleep")
            || msg.contains("UserPromptSubmit")
            || msg.contains("Forked agent [prompt_suggestion]")
        {
            last_end = Some(i);
        }
    }

    match (last_start, last_end) {
        (Some(s), Some(e)) => s > e, // stream started after the last end marker
        (Some(_), None) => true,     // stream started, never ended in this tail
        _ => false,
    }
}

/// Pure classifier — no I/O.
pub fn classify(log: &SessionLog, fs: &FsSignals) -> SessionState {
    // 1. Strongest signal: awaiting patterns in recent log lines
    if let Some(reason) = detect_awaiting_signal(log) {
        return SessionState::AwaitingInput { reason: Some(reason) };
    }

    // 2. Prompt suggestion subagent active or finished = main response is complete → Done.
    //    Covers the entire subagent window to prevent Running↔Done flicker.
    if in_prompt_suggestion_phase(log) {
        return SessionState::Done;
    }

    let secs_idle = fs.secs_since_last_write().unwrap_or(f64::MAX);

    // 3. Mid-stream: "Stream started" seen but response not yet finished → Running
    if mid_stream(log) {
        return SessionState::Running;
    }

    // 4. Very recent write = still running (tight window — mid_stream covers the rest)
    if secs_idle < 2.0 {
        return SessionState::Running;
    }

    // 4. Moderately recent + permission context in tail
    if secs_idle < 30.0 && tail_ends_with_permission_context(log) {
        return SessionState::AwaitingInput { reason: None };
    }

    // 5. 5-30s window with no permission context: use process probe to resolve ambiguity.
    //    If no process holds the file, the session is done. If held, still running/idle.
    if secs_idle < 30.0 {
        return match fs.process_holds_file {
            Some(false) => SessionState::Done,
            Some(true) | None => SessionState::Idle,
        };
    }

    // 6-7. Quiescent (>30s) — use process probe
    match fs.process_holds_file {
        Some(true) => SessionState::Idle,
        Some(false) => SessionState::Done,
        None => SessionState::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;
    use chrono::Utc;
    use vigil_core::{LogLevel, LogLine};

    fn make_log(messages: &[&str]) -> SessionLog {
        let lines: Vec<LogLine> = messages
            .iter()
            .map(|m| LogLine {
                timestamp: Utc::now(),
                level: LogLevel::Info,
                component: None,
                message: m.to_string(),
            })
            .collect();
        let last_ts = lines.last().map(|l| l.timestamp);
        SessionLog {
            tail: lines,
            last_line_ts: last_ts,
            file_size_bytes: 0,
        }
    }

    fn fs_recent() -> FsSignals {
        let now = SystemTime::now();
        FsSignals {
            sampled_at: now,
            log_mtime: now,
            process_holds_file: None,
        }
    }

    fn fs_quiescent(held: Option<bool>) -> FsSignals {
        let mtime = SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(60))
            .unwrap();
        FsSignals {
            sampled_at: SystemTime::now(),
            log_mtime: mtime,
            process_holds_file: held,
        }
    }

    #[test]
    fn permission_prompt_detected() {
        let log = make_log(&["Notification with query: permission_prompt"]);
        let state = classify(&log, &fs_quiescent(Some(true)));
        assert!(matches!(state, SessionState::AwaitingInput { .. }));
        assert!(state.needs_attention());
    }

    #[test]
    fn user_prompt_submit_clears_awaiting() {
        let log = make_log(&[
            "Notification with query: permission_prompt",
            "UserPromptSubmit",
        ]);
        let state = classify(&log, &fs_quiescent(Some(true)));
        // After UserPromptSubmit, awaiting signal should not be triggered; process holds = Idle
        assert!(matches!(state, SessionState::Idle));
    }

    #[test]
    fn recent_write_is_running() {
        let log = make_log(&["some normal log"]);
        let state = classify(&log, &fs_recent());
        assert!(matches!(state, SessionState::Running));
    }

    #[test]
    fn quiescent_not_held_is_done() {
        let log = make_log(&["some normal log"]);
        let state = classify(&log, &fs_quiescent(Some(false)));
        assert!(matches!(state, SessionState::Done));
    }

    #[test]
    fn quiescent_held_is_idle() {
        let log = make_log(&["some normal log"]);
        let state = classify(&log, &fs_quiescent(Some(true)));
        assert!(matches!(state, SessionState::Idle));
    }

    #[test]
    fn on_cancel_tool_use_detected() {
        let log = make_log(&["[onCancel] cleaning up streamMode=tool-use context"]);
        let state = classify(&log, &fs_quiescent(Some(false)));
        assert!(matches!(state, SessionState::AwaitingInput { .. }));
    }
}
