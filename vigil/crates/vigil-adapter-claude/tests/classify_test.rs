use std::time::SystemTime;
use vigil_adapter_claude::{classifier, log_parser};
use vigil_core::{FsSignals, SessionState};

fn fs_old(held: Option<bool>) -> FsSignals {
    let old_mtime = SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(120))
        .unwrap();
    FsSignals {
        sampled_at: SystemTime::now(),
        log_mtime: old_mtime,
        process_holds_file: held,
    }
}

fn fs_fresh() -> FsSignals {
    let now = SystemTime::now();
    FsSignals {
        sampled_at: now,
        log_mtime: now,
        process_holds_file: None,
    }
}

fn load_fixture(rel_path: &str) -> vigil_core::SessionLog {
    let text = std::fs::read_to_string(
        std::path::Path::new("tests/fixtures").join(rel_path),
    )
    .unwrap_or_else(|_| panic!("fixture not found: {rel_path}"));
    log_parser::parse_lines(&text, 200);
    let tail = log_parser::parse_lines(&text, 200);
    let last_line_ts = tail.last().map(|l| l.timestamp);
    vigil_core::SessionLog {
        tail,
        last_line_ts,
        file_size_bytes: text.len() as u64,
    }
}

#[test]
fn awaiting_permission_fixture_classifies_as_awaiting() {
    let log = load_fixture("awaiting_permission/session-awaiting.txt");
    // File is old but has permission hook signals in tail
    let state = classifier::classify(&log, &fs_old(Some(false)));
    assert!(
        matches!(state, SessionState::AwaitingInput { .. }),
        "expected AwaitingInput, got {state:?}"
    );
}

#[test]
fn awaiting_notify_fixture_classifies_as_awaiting() {
    let log = load_fixture("awaiting_notify/session-awaiting-notify.txt");
    let state = classifier::classify(&log, &fs_old(Some(false)));
    assert!(
        matches!(state, SessionState::AwaitingInput { .. }),
        "expected AwaitingInput, got {state:?}"
    );
}

#[test]
fn idle_fixture_with_held_process_classifies_as_idle() {
    let log = load_fixture("idle/session-idle.txt");
    let state = classifier::classify(&log, &fs_old(Some(true)));
    assert!(matches!(state, SessionState::Idle), "expected Idle, got {state:?}");
}

#[test]
fn done_fixture_without_held_process_classifies_as_done() {
    let log = load_fixture("done/session-done.txt");
    let state = classifier::classify(&log, &fs_old(Some(false)));
    assert!(matches!(state, SessionState::Done), "expected Done, got {state:?}");
}

#[test]
fn fresh_file_classifies_as_running() {
    let log = load_fixture("idle/session-idle.txt");
    let state = classifier::classify(&log, &fs_fresh());
    assert!(
        matches!(state, SessionState::Running),
        "expected Running, got {state:?}"
    );
}
