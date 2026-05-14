use anyhow::{anyhow, Context, Result};
use diffdesk_core::{
    canceled_path, create_session_from_args, help_text, parse_args, result_path, spawn_app, ParsedArgs,
};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    if let Err(error) = run() {
        eprintln!("diffdesk: {error:#}");
        std::process::exit(2);
    }
}

fn run() -> Result<()> {
    let raw_args = env::args().skip(1).collect::<Vec<_>>();
    let args = parse_args(&raw_args)?;
    if args.help {
        print!("{}", help_text());
        return Ok(());
    }

    let current_dir = env::current_dir().context("failed to read current directory")?;
    let session = create_session_from_args(&args, &current_dir)?;
    println!("Created Diffdesk session {}", session.session_id);
    println!("Session dir: {}", session.session_dir.display());

    let command = resolve_app_command(&args)?;
    spawn_app(&command, &session.session_id, false)?;

    if !args.wait {
        println!("Launched Diffdesk without waiting.");
        return Ok(());
    }

    wait_for_result(&session.session_id)?;
    let result_json = fs::read_to_string(result_path(&session.session_id)?)?;
    let value: serde_json::Value = serde_json::from_str(&result_json)?;
    if let Some(path) = value.get("outputPath").and_then(|v| v.as_str()) {
        println!("Review written to {path}");
    } else {
        println!("Review submitted.");
    }
    Ok(())
}

fn resolve_app_command(args: &ParsedArgs) -> Result<String> {
    if let Some(command) = args.app_command.clone() {
        return Ok(command);
    }
    if let Ok(command) = env::var("DIFFDESK_APP") {
        if !command.trim().is_empty() {
            return Ok(command);
        }
    }

    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let bundled_app = parent.join("bundle/macos/Diffdesk.app");
            if bundled_app.exists() {
                return Ok(format!("/usr/bin/open -n {} --args", bundled_app.display()));
            }

            let sibling_app = parent.join("diffdesk-app");
            if sibling_app.exists() {
                return Ok(sibling_app.display().to_string());
            }
        }
    }

    let release_bundle = PathBuf::from("target/release/bundle/macos/Diffdesk.app");
    if release_bundle.exists() {
        return Ok(format!("/usr/bin/open -n {} --args", release_bundle.display()));
    }

    let debug_app = PathBuf::from("target/debug/diffdesk-app");
    if debug_app.exists() {
        return Ok(debug_app.display().to_string());
    }

    let release_app = PathBuf::from("target/release/diffdesk-app");
    if release_app.exists() {
        return Ok(release_app.display().to_string());
    }

    Err(anyhow!(
        "could not find Diffdesk app binary. Build/run the app first, or pass --app-command <path>, or set DIFFDESK_APP. In development, run: pnpm dev -- --session <session-id>"
    ))
}

fn wait_for_result(session_id: &str) -> Result<()> {
    let result = result_path(session_id)?;
    let canceled = canceled_path(session_id)?;
    let started = Instant::now();
    println!("Waiting for review submission...");

    loop {
        if result.exists() {
            return Ok(());
        }
        if canceled.exists() {
            return Err(anyhow!("review canceled"));
        }
        if started.elapsed() > Duration::from_secs(60 * 60 * 12) {
            return Err(anyhow!("timed out waiting for review result"));
        }
        thread::sleep(Duration::from_millis(250));
    }
}
