use std::path::{Path, PathBuf};
use vigil_core::{SessionId, VigilError};

/// UUID pattern: 8-4-4-4-12 hex chars
fn is_uuid_filename(name: &str) -> bool {
    let name = match name.strip_suffix(".txt") {
        Some(n) => n,
        None => return false,
    };
    let parts: Vec<&str> = name.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let expected_lens = [8, 4, 4, 4, 12];
    parts
        .iter()
        .zip(expected_lens.iter())
        .all(|(p, &len)| p.len() == len && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Scan debug_dir for UUID.txt files, sorted newest-first by mtime.
pub async fn discover_sessions(debug_dir: &Path) -> Result<Vec<SessionId>, VigilError> {
    let mut entries: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();

    let mut dir = tokio::fs::read_dir(debug_dir).await.map_err(|e| {
        VigilError::Io(e)
    })?;

    while let Some(entry) = dir.next_entry().await? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !is_uuid_filename(&name_str) {
            continue;
        }
        let mtime = entry.metadata().await?.modified()?;
        entries.push((mtime, entry.path()));
    }

    // Sort newest first
    entries.sort_by(|a, b| b.0.cmp(&a.0));

    let ids = entries
        .into_iter()
        .filter_map(|(_, path)| {
            path.file_stem()
                .map(|s| SessionId(s.to_string_lossy().into_owned()))
        })
        .collect();

    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_filename_valid() {
        assert!(is_uuid_filename(
            "a032f5c1-f7ab-462f-9005-c924c16bf31a.txt"
        ));
    }

    #[test]
    fn uuid_filename_invalid() {
        assert!(!is_uuid_filename("not-a-uuid.txt"));
        assert!(!is_uuid_filename("a032f5c1-f7ab-462f-9005-c924c16bf31a.log"));
        assert!(!is_uuid_filename("session.txt"));
    }
}
