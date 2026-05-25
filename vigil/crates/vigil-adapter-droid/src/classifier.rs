use vigil_core::{FsSignals, SessionState};

enum LastRole {
    UserText,
    AssistantText,
}

/// Scan JSONL lines (newest last) to find the last significant message role.
/// - Skips visibility: llm_only context injections.
/// - Skips user messages whose content is exclusively tool_result blocks.
fn last_significant_role(content: &str) -> Option<LastRole> {
    let mut last = None;
    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if val["type"].as_str() != Some("message") {
            continue;
        }
        if val["visibility"].as_str() == Some("llm_only") {
            continue;
        }

        match val["message"]["role"].as_str().unwrap_or("") {
            "assistant" => {
                let has_text = val["message"]["content"]
                    .as_array()
                    .map(|arr| arr.iter().any(|b| b["type"].as_str() == Some("text")))
                    .unwrap_or(false);
                if has_text {
                    last = Some(LastRole::AssistantText);
                }
            }
            "user" => {
                let is_only_tool_results = val["message"]["content"]
                    .as_array()
                    .map(|arr| {
                        !arr.is_empty()
                            && arr
                                .iter()
                                .all(|b| b["type"].as_str() == Some("tool_result"))
                    })
                    .unwrap_or(false);
                if !is_only_tool_results {
                    last = Some(LastRole::UserText);
                }
            }
            _ => {}
        }
    }
    last
}

pub fn classify(content: &str, fs: &FsSignals) -> SessionState {
    let secs = fs.secs_since_last_write();

    // File written in the last 3 seconds - something is actively happening.
    if secs.map(|s| s < 3.0).unwrap_or(false) {
        return SessionState::Running;
    }

    match last_significant_role(content) {
        // User sent a message and droid has not responded yet.
        Some(LastRole::UserText) if secs.map(|s| s < 60.0).unwrap_or(false) => {
            SessionState::Running
        }
        // Droid replied - session is idle awaiting next user message.
        Some(LastRole::AssistantText) => SessionState::Done,
        // Ambiguous: use recency as a tie-breaker.
        _ => {
            if secs.map(|s| s < 30.0).unwrap_or(false) {
                SessionState::Running
            } else {
                SessionState::Done
            }
        }
    }
}
