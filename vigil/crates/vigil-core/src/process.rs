use crate::VigilError;
use std::path::{Path, PathBuf};

/// Find the PID of a running process named exactly `process_name` whose current
/// working directory is `dir`. Uses `pgrep -x` for exact name match, then
/// `lsof` to resolve each PID's cwd.
pub async fn get_pid_for_process_in_dir(process_name: &str, dir: &Path) -> Option<u32> {
    // Exact-name match avoids prefix collisions (e.g. "pi" matching "pidinfo").
    let pgrep = tokio::process::Command::new("pgrep")
        .args(["-x", process_name])
        .output()
        .await
        .ok()?;

    let pids: Vec<String> = String::from_utf8_lossy(&pgrep.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();

    if pids.is_empty() {
        return None;
    }

    // Build a single lsof call for all matching PIDs.
    let mut args = vec!["-a", "-d", "cwd", "-F", "pn"];
    for pid in &pids {
        args.push("-p");
        args.push(pid.as_str());
    }

    let lsof = tokio::process::Command::new("lsof")
        .args(&args)
        .output()
        .await
        .ok()?;

    let text = String::from_utf8_lossy(&lsof.stdout);
    let dir_str = dir.to_string_lossy();

    let mut current_pid: Option<u32> = None;
    for line in text.lines() {
        if let Some(pid_str) = line.strip_prefix('p') {
            current_pid = pid_str.parse().ok();
        } else if let Some(cwd) = line.strip_prefix('n') {
            if cwd == dir_str {
                return current_pid;
            }
        }
    }
    None
}

/// A process whose current working directory was observed by `cwd_snapshot`.
#[derive(Debug, Clone)]
pub struct CwdProcess {
    pub pid: u32,
    pub command: String,
    pub cwd: PathBuf,
}

/// Single global `lsof -d cwd -F pcn` snapshot of every process's cwd. Callers
/// filter the result per-container, which is far cheaper than running lsof once
/// per worktree.
pub async fn cwd_snapshot() -> Vec<CwdProcess> {
    let Ok(output) = tokio::process::Command::new("lsof")
        .args(["-d", "cwd", "-F", "pcn", "-w"])
        .output()
        .await
    else {
        return vec![];
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let mut out = Vec::new();
    let mut pid: Option<u32> = None;
    let mut cmd: Option<String> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix('p') {
            pid = rest.parse().ok();
            cmd = None;
        } else if let Some(rest) = line.strip_prefix('c') {
            cmd = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix('n') {
            if let (Some(pid), Some(cmd)) = (pid, cmd.clone()) {
                out.push(CwdProcess {
                    pid,
                    command: cmd,
                    cwd: PathBuf::from(rest),
                });
            }
        }
    }
    out
}

/// Return all PIDs currently holding `path` open, via `lsof -F p`.
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

/// Find a PID's controlling TTY via `ps -p <pid> -o tty=` and write `msg\n` to it.
pub async fn send_to_tty(pid: u32, msg: &str) -> Result<(), VigilError> {
    let out = tokio::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "tty="])
        .output()
        .await
        .map_err(|e| VigilError::ProcessProbe(e.to_string()))?;

    let tty = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if tty == "??" || tty.is_empty() {
        return Err(VigilError::NotSupported(
            "process has no controlling TTY".into(),
        ));
    }

    let tty_path = format!("/dev/{tty}");
    let payload = format!("{msg}\n");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&tty_path)
        .and_then(|mut f| {
            use std::io::Write as _;
            f.write_all(payload.as_bytes())
        })
        .map_err(|e| VigilError::ProcessProbe(format!("write to {tty_path}: {e}")))?;
    Ok(())
}
