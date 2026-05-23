use vigil_adapter_pi::classifier::{classify, parse_last_message, LastMessage};
use vigil_core::SessionState;

// ── parse_last_message ────────────────────────────────────────────────────────

#[test]
fn detects_assistant_text() {
    let content = include_str!("fixtures/awaiting.jsonl");
    assert_eq!(parse_last_message(content), LastMessage::AssistantText);
}

#[test]
fn detects_tool_result() {
    let content = include_str!("fixtures/running_tool_result.jsonl");
    assert_eq!(parse_last_message(content), LastMessage::ToolResult);
}

#[test]
fn detects_assistant_tool_call() {
    let content = include_str!("fixtures/running_tool_call.jsonl");
    assert_eq!(parse_last_message(content), LastMessage::AssistantToolCall);
}

#[test]
fn detects_user_message() {
    let content = include_str!("fixtures/running_user.jsonl");
    assert_eq!(parse_last_message(content), LastMessage::User);
}

#[test]
fn unknown_on_empty() {
    assert_eq!(parse_last_message(""), LastMessage::Unknown);
    assert_eq!(parse_last_message("\n\n"), LastMessage::Unknown);
}

// ── classify ─────────────────────────────────────────────────────────────────

#[test]
fn awaiting_when_assistant_text_and_idle() {
    assert_eq!(
        classify(LastMessage::AssistantText, 60.0),
        SessionState::AwaitingInput { reason: None }
    );
}

#[test]
fn awaiting_even_when_stale() {
    // AssistantText always means AwaitingInput — Pi finished its turn regardless of age
    assert_eq!(
        classify(LastMessage::AssistantText, 3600.0),
        SessionState::AwaitingInput { reason: None }
    );
}

#[test]
fn running_when_tool_result_recent() {
    assert_eq!(
        classify(LastMessage::ToolResult, 2.0),
        SessionState::Running
    );
    assert_eq!(
        classify(LastMessage::ToolResult, 10.0),
        SessionState::Running
    );
}

#[test]
fn idle_when_tool_result_stale() {
    assert_eq!(classify(LastMessage::ToolResult, 45.0), SessionState::Idle);
}

#[test]
fn running_when_tool_call_recent() {
    assert_eq!(
        classify(LastMessage::AssistantToolCall, 5.0),
        SessionState::Running
    );
}

#[test]
fn idle_when_tool_call_stale() {
    assert_eq!(
        classify(LastMessage::AssistantToolCall, 60.0),
        SessionState::Idle
    );
}

#[test]
fn running_when_user_message_recent() {
    assert_eq!(classify(LastMessage::User, 5.0), SessionState::Running);
}

#[test]
fn idle_when_user_message_stale() {
    assert_eq!(classify(LastMessage::User, 45.0), SessionState::Idle);
}

#[test]
fn always_running_when_file_just_written() {
    // < 3s mtime means the file is being actively written regardless of message kind
    assert_eq!(
        classify(LastMessage::AssistantText, 1.0),
        SessionState::Running
    );
    assert_eq!(classify(LastMessage::Unknown, 1.0), SessionState::Running);
}
