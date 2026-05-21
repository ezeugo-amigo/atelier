use std::collections::HashMap;
use std::path::{Path, PathBuf};
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use vigil_core::{SessionId, VigilError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub display: Option<String>,
    #[serde(rename = "pastedContents")]
    pub pasted_contents: Option<serde_json::Value>,
    /// Unix milliseconds
    pub timestamp: Option<i64>,
    pub project: Option<String>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionHistory {
    pub entries: Vec<HistoryEntry>,
}

impl SessionHistory {
    pub fn first_timestamp(&self) -> Option<DateTime<Utc>> {
        self.entries
            .iter()
            .filter_map(|e| e.timestamp)
            .min()
            .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
    }

    pub fn last_timestamp(&self) -> Option<DateTime<Utc>> {
        self.entries
            .iter()
            .filter_map(|e| e.timestamp)
            .max()
            .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
    }

    pub fn last_display(&self) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find_map(|e| e.display.as_deref().filter(|s| !s.is_empty()))
    }

    pub fn project_dir(&self) -> Option<PathBuf> {
        self.entries
            .iter()
            .rev()
            .find_map(|e| e.project.as_deref())
            .map(PathBuf::from)
    }
}

/// Find the most recent session whose project dir matches `dir`.
pub fn find_session_for_dir<'a>(
    history: &'a HashMap<SessionId, SessionHistory>,
    dir: &Path,
) -> Option<(&'a SessionId, &'a SessionHistory)> {
    history
        .iter()
        .filter(|(_, h)| h.project_dir().as_deref() == Some(dir))
        .max_by_key(|(_, h)| h.last_timestamp())
}

pub async fn load_history(
    path: &Path,
) -> Result<HashMap<SessionId, SessionHistory>, VigilError> {
    let content = tokio::fs::read_to_string(path).await?;
    let mut map: HashMap<SessionId, SessionHistory> = HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let entry: HistoryEntry = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(err) => {
                tracing::debug!("skipping malformed history line: {err}");
                continue;
            }
        };
        let session_id = match &entry.session_id {
            Some(id) if !id.is_empty() => SessionId(id.clone()),
            _ => continue,
        };
        map.entry(session_id).or_insert_with(|| SessionHistory { entries: Vec::new() })
            .entries
            .push(entry);
    }

    Ok(map)
}
