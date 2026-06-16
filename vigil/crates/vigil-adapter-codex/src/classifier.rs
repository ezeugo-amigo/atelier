use vigil_core::SessionState;

#[derive(Debug, PartialEq, Eq)]
pub enum LastMessage {
    /// User sent a message; Codex is (or should be) processing it.
    User,
    /// Codex finished its turn with a text response; waiting for user input.
    AssistantText,
    /// Could not determine (empty session, only system messages, etc.).
    Unknown,
}

/// Parse the last meaningful message from a Codex session JSONL.
///
/// Codex JSONL lines are `{"type": "response_item", "payload": {"role": "...", "content": [...]}}`.
/// We skip `developer` role (system injections), `event_msg`, `turn_context`, `compacted`, and
/// null-role items as they don't represent conversational turns.
pub fn parse_last_message(content: &str) -> LastMessage {
    for line in content.lines().rev() {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if val["type"].as_str() != Some("response_item") {
            continue;
        }
        match val["payload"]["role"].as_str() {
            Some("user") => return LastMessage::User,
            Some("assistant") => return LastMessage::AssistantText,
            // developer / null role — skip
            _ => continue,
        }
    }
    LastMessage::Unknown
}

pub fn classify(last: LastMessage, secs_since_write: f64) -> SessionState {
    if secs_since_write < 3.0 {
        return SessionState::Running;
    }
    match last {
        LastMessage::User => {
            if secs_since_write < 60.0 {
                SessionState::Running
            } else {
                SessionState::Idle
            }
        }
        LastMessage::AssistantText => SessionState::AwaitingInput { reason: None },
        LastMessage::Unknown => {
            if secs_since_write < 30.0 {
                SessionState::Idle
            } else {
                SessionState::Done
            }
        }
    }
}
