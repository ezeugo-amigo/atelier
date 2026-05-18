use std::path::Path;

/// Return all PIDs that currently hold `path` open, via `lsof -F p`.
pub async fn get_pids_holding_file(path: &Path) -> Vec<u32> {
    let Ok(output) = tokio::process::Command::new("lsof")
        .args(["-F", "p", "-w"])
        .arg(path)
        .output()
        .await
    else {
        return vec![];
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix('p')?.parse().ok())
        .collect()
}

/// Check whether any process holds the file open using `lsof`.
/// Returns `Some(true)` if held, `Some(false)` if confirmed not held, `None` on spawn failure.
pub async fn file_is_held_open(path: &Path) -> Option<bool> {
    let result = tokio::process::Command::new("lsof")
        .args(["-F", "p", "-w"])
        .arg(path)
        .output()
        .await;

    match result {
        Err(e) => {
            tracing::warn!("lsof spawn failed for {}: {e}", path.display());
            None
        }
        Ok(output) => {
            let held = !output.stdout.is_empty();
            tracing::debug!(
                "lsof for {}: status={} stdout_bytes={} held={held}",
                path.display(),
                output.status,
                output.stdout.len()
            );
            Some(held)
        }
    }
}
