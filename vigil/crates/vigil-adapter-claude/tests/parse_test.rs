use vigil_adapter_claude::log_parser::{parse_lines, parse_log_line};
use vigil_core::LogLevel;

#[test]
fn parse_full_line_with_component() {
    let line = "2026-05-17T18:14:44.934Z [DEBUG] [useDeferredValue] Messages deferred by 1 (26→27)";
    let parsed = parse_log_line(line).unwrap();
    assert_eq!(parsed.level, LogLevel::Debug);
    assert_eq!(parsed.component.as_deref(), Some("useDeferredValue"));
    assert!(parsed.message.contains("Messages deferred"));
}

#[test]
fn parse_line_without_component() {
    let line = "2026-05-17T18:15:09.183Z [DEBUG] Getting matching hook commands for Notification with query: permission_prompt";
    let parsed = parse_log_line(line).unwrap();
    assert_eq!(parsed.level, LogLevel::Debug);
    assert!(parsed.component.is_none());
    assert!(parsed.message.contains("permission_prompt"));
}

#[test]
fn skip_continuation_lines() {
    assert!(parse_log_line("    \"behavior\": \"allow\",").is_none());
    assert!(parse_log_line("  }").is_none());
    assert!(parse_log_line("]").is_none());
}

#[test]
fn parse_permission_suggestions_line() {
    let line = "2026-05-17T18:16:11.220Z [DEBUG] Permission suggestions for Bash: [";
    let parsed = parse_log_line(line).unwrap();
    assert!(parsed
        .message
        .starts_with("Permission suggestions for Bash:"));
}

#[test]
fn parse_real_fixture_file() {
    let fixture =
        std::fs::read_to_string("tests/fixtures/awaiting_permission/session-awaiting.txt")
            .expect("fixture file should exist");

    let lines = parse_lines(&fixture, 200);
    // Should have parsed some lines (skipping JSON continuation lines)
    assert!(!lines.is_empty(), "should parse at least one log line");

    // Should contain the executePermissionRequestHooks line
    let has_hooks = lines
        .iter()
        .any(|l| l.message.contains("executePermissionRequestHooks"));
    assert!(has_hooks, "fixture should contain permission hook line");
}

#[test]
fn parse_awaiting_notify_fixture() {
    let fixture =
        std::fs::read_to_string("tests/fixtures/awaiting_notify/session-awaiting-notify.txt")
            .expect("fixture file should exist");

    let lines = parse_lines(&fixture, 200);
    assert!(!lines.is_empty());
    let has_notify = lines
        .iter()
        .any(|l| l.message.contains("permission_prompt"));
    assert!(has_notify);
}
