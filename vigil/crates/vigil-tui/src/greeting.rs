//! A lightweight startup greeting for the Vigil dashboard.

use std::process::Stdio;
use std::time::Duration;

use chrono::{Local, Timelike};

const GREETING_MODEL: &str = "haiku";
const GREETING_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_NAME_CHARS: usize = 80;
const MAX_GREETING_CHARS: usize = 180;

#[derive(Debug, Clone)]
pub enum Greeting {
    Ready(String),
}

/// Resolve a friendly local name without requiring any additional settings.
pub fn user_name() -> String {
    std::env::var("VIGIL_USER_NAME")
        .ok()
        .or_else(|| git_user_name(&["config", "--get", "user.name"]))
        .or_else(|| git_user_name(&["config", "--global", "--get", "user.name"]))
        .or_else(|| std::env::var("USER").ok())
        .map(|name| clean_name(&name))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "there".to_string())
}

fn git_user_name(args: &[&str]) -> Option<String> {
    std::process::Command::new("git")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .map(|name| clean_name(&name))
        .filter(|name| !name.is_empty())
}

fn clean_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_NAME_CHARS)
        .collect()
}

fn time_of_day() -> &'static str {
    match Local::now().hour() {
        0..=11 => "morning",
        12..=17 => "afternoon",
        _ => "evening",
    }
}

fn prompt_for(name: &str) -> String {
    format!(
        "Come up with one short, clean coding-related joke and use it to greet the developer named {name:?}. Address them by name, use a natural {time} salutation when it fits, and keep the result warm and concise. Return exactly two sentences, with no markdown, quotation marks, preamble, or question.",
        time = time_of_day(),
    )
}

fn normalize_output(output: &str) -> String {
    let compact = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let unquoted = compact
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .unwrap_or(&compact)
        .trim();
    unquoted.chars().take(MAX_GREETING_CHARS).collect()
}

/// Produce a deterministic greeting when the local model is unavailable.
pub fn fallback(name: &str) -> String {
    let salutation = match time_of_day() {
        "morning" => "Good morning",
        "afternoon" => "Good afternoon",
        _ => "Good evening",
    };
    format!("{salutation}, {name}. Vigil is online and keeping watch over your coding sessions.")
}

/// Generate a greeting by shelling out to the same small model used by recap.
pub async fn generate(name: &str) -> Result<String, String> {
    let prompt = prompt_for(name);
    let call = tokio::process::Command::new("claude")
        .arg("--print")
        .arg("--model")
        .arg(GREETING_MODEL)
        .arg(&prompt)
        .env_remove("CLAUDECODE")
        .current_dir(std::env::temp_dir())
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output = match tokio::time::timeout(GREETING_TIMEOUT, call).await {
        Ok(result) => result.map_err(|error| format!("claude spawn failed: {error}"))?,
        Err(_) => return Err("timed out after 20s".to_string()),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr
            .trim()
            .lines()
            .next()
            .unwrap_or("claude exited with error");
        return Err(message.chars().take(120).collect());
    }

    let text = normalize_output(&String::from_utf8_lossy(&output.stdout));
    if text.is_empty() {
        return Err("empty greeting".to_string());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_names_for_display_and_prompt() {
        assert_eq!(clean_name("  Ada\n  Lovelace  "), "Ada Lovelace");
    }

    #[test]
    fn normalizes_model_output_to_one_line() {
        assert_eq!(normalize_output("\"Hello, Ada.\"\n"), "Hello, Ada.");
    }

    #[test]
    fn fallback_is_personalized() {
        assert!(fallback("Ada").contains("Ada"));
        assert!(fallback("Ada").contains("Vigil is online"));
    }

    #[test]
    fn prompt_names_the_person_and_requests_a_joke() {
        let prompt = prompt_for("Ada Lovelace");
        assert!(prompt.contains("Ada Lovelace"));
        assert!(prompt.contains("coding-related joke"));
        assert!(!prompt.contains("You are Vigil"));
    }
}
