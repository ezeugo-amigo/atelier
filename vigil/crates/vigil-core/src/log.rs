use std::time::SystemTime;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub component: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct SessionLog {
    pub tail: Vec<LogLine>,
    pub last_line_ts: Option<DateTime<Utc>>,
    pub file_size_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct FsSignals {
    pub sampled_at: SystemTime,
    pub log_mtime: SystemTime,
    pub process_holds_file: Option<bool>,
}

impl FsSignals {
    pub fn secs_since_last_write(&self) -> Option<f64> {
        self.sampled_at
            .duration_since(self.log_mtime)
            .ok()
            .map(|d| d.as_secs_f64())
    }
}
