use vigil_core::SessionState;

/// What the last message in a Pi session file tells us about current state.
#[derive(Debug, PartialEq, Eq)]
pub enum LastMessage {
    /// User sent a message; Pi is processing it.
    User,
    /// Pi finished its turn with a text response; waiting for user input.
    AssistantText,
    /// Pi issued a tool call as its last action; tool result pending.
    AssistantToolCall,
    /// A tool result was appended; Pi is processing it.
    ToolResult,
    /// Could not determine.
    Unknown,
}

/// Parse the last meaningful message from JSONL session content.
pub fn parse_last_message(content: &str) -> LastMessage {
    let last_line = content.lines().rev().find(|l| !l.trim().is_empty());

    let Some(line) = last_line else {
        return LastMessage::Unknown;
    };

    let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
        return LastMessage::Unknown;
    };

    let msg = &val["message"];
    match msg["role"].as_str() {
        Some("user") => LastMessage::User,
        Some("toolResult") => LastMessage::ToolResult,
        Some("assistant") => {
            // The last item in content[] determines what Pi did last
            let last_type = msg["content"]
                .as_array()
                .and_then(|a| a.last())
                .and_then(|c| c["type"].as_str())
                .unwrap_or("");
            if last_type == "toolCall" {
                LastMessage::AssistantToolCall
            } else {
                LastMessage::AssistantText
            }
        }
        _ => LastMessage::Unknown,
    }
}

/// Derive session state from the last message kind and how long ago the file was written.
pub fn classify(last: LastMessage, secs_since_write: f64) -> SessionState {
    // File being actively written → always Running
    if secs_since_write < 3.0 {
        return SessionState::Running;
    }

    match last {
        // Pi is mid-turn (tool in flight or just received user input)
        LastMessage::User | LastMessage::ToolResult | LastMessage::AssistantToolCall => {
            if secs_since_write < 30.0 {
                SessionState::Running
            } else {
                // Stalled: tool took too long or process died mid-turn
                SessionState::Idle
            }
        }
        // Pi finished its response, session open, waiting for user
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
