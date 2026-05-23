use chrono::{DateTime, Utc};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use vigil_core::{LogLevel, LogLine, SessionLog, VigilError};

const READ_TAIL_BYTES: u64 = 65536; // 64 KB

pub async fn read_log_tail(path: &Path, n: usize) -> Result<SessionLog, VigilError> {
    let mut file = std::fs::File::open(path)?;
    let file_size_bytes = file.metadata()?.len();

    let offset = file_size_bytes.saturating_sub(READ_TAIL_BYTES);
    file.seek(SeekFrom::Start(offset))?;

    let mut buf = String::new();
    file.read_to_string(&mut buf)?;

    let tail = parse_lines(&buf, n);
    let last_line_ts = tail.last().map(|l| l.timestamp);

    Ok(SessionLog {
        tail,
        last_line_ts,
        file_size_bytes,
    })
}

pub fn parse_lines(text: &str, n: usize) -> Vec<LogLine> {
    let lines: Vec<&str> = text.lines().collect();
    let mut result = Vec::new();

    for line in &lines {
        if let Some(parsed) = parse_log_line(line) {
            result.push(parsed);
        }
        // continuation lines (no leading timestamp) are skipped
    }

    // Return last n
    if result.len() > n {
        result.drain(0..result.len() - n);
    }
    result
}

/// Parse a single Claude Code debug log line:
/// `{ISO8601Z} [{LEVEL}] [{COMPONENT}?] {message}`
pub fn parse_log_line(raw: &str) -> Option<LogLine> {
    // Must start with a digit (year)
    if !raw.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }

    // Find the timestamp: everything up to the first space
    let space = raw.find(' ')?;
    let ts_str = &raw[..space];
    let timestamp: DateTime<Utc> = ts_str.parse().ok()?;

    let rest = raw[space + 1..].trim_start();

    // Parse [LEVEL]
    let rest = rest.strip_prefix('[')?;
    let bracket_end = rest.find(']')?;
    let level_str = &rest[..bracket_end];
    let level = parse_level(level_str)?;
    let rest = rest[bracket_end + 1..].trim_start();

    // Optional [COMPONENT]
    let (component, message) = if rest.starts_with('[') {
        let rest = &rest[1..];
        if let Some(end) = rest.find(']') {
            let comp = rest[..end].to_string();
            let msg = rest[end + 1..].trim_start().to_string();
            (Some(comp), msg)
        } else {
            (None, rest.to_string())
        }
    } else {
        (None, rest.to_string())
    };

    Some(LogLine {
        timestamp,
        level,
        component,
        message,
    })
}

fn parse_level(s: &str) -> Option<LogLevel> {
    match s.to_ascii_uppercase().as_str() {
        "DEBUG" => Some(LogLevel::Debug),
        "INFO" => Some(LogLevel::Info),
        "WARN" | "WARNING" => Some(LogLevel::Warn),
        "ERROR" => Some(LogLevel::Error),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_line() {
        let line = "2024-01-15T10:30:00.000Z [INFO] [ToolUse] Executing bash tool";
        let parsed = parse_log_line(line).unwrap();
        assert_eq!(parsed.level, LogLevel::Info);
        assert_eq!(parsed.component.as_deref(), Some("ToolUse"));
        assert_eq!(parsed.message, "Executing bash tool");
    }

    #[test]
    fn parses_line_without_component() {
        let line = "2024-01-15T10:30:00.000Z [DEBUG] Some message here";
        let parsed = parse_log_line(line).unwrap();
        assert_eq!(parsed.level, LogLevel::Debug);
        assert!(parsed.component.is_none());
        assert_eq!(parsed.message, "Some message here");
    }

    #[test]
    fn skips_continuation_lines() {
        let line = "    at Object.<anonymous> (index.js:10:5)";
        assert!(parse_log_line(line).is_none());
    }

    #[test]
    fn parse_lines_returns_last_n() {
        let text = (0..100)
            .map(|i| format!("2024-01-15T10:{:02}:00.000Z [INFO] line {}", i % 60, i))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = parse_lines(&text, 10);
        assert_eq!(lines.len(), 10);
    }
}
