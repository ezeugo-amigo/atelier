//! Agent-suggested container names.
//!
//! Pressing `m` on a container asks `claude --print` to read the same
//! transcript window used for recaps and propose a short display name that
//! reflects the scope of work, instead of the random `adjective-noun` id
//! every container starts with.

use std::time::Duration;

use vigil_core::LogEvent;

use crate::recap::{ask, build_transcript};

/// Shorter than the recap timeout — a name is a few words, not a paragraph.
const RENAME_TIMEOUT: Duration = Duration::from_secs(20);
/// Cap on the returned name so a runaway response can't wreck the list layout.
const MAX_NAME_CHARS: usize = 60;

const PROMPT_PREAMBLE: &str = "You are naming a coding-agent session for a developer who manages \
several sessions at once and needs to tell them apart at a glance. Read the transcript below and \
reply with ONLY a short display name (3-6 words, plain words, no quotes, no punctuation, no \
markdown) that describes the concrete scope of work. Here is the recent transcript:\n\n";

/// Ask `claude --print` to suggest a container name from its recent transcript.
/// Returns the suggested name on success, or a short human-readable error
/// string on failure.
pub async fn suggest_name(events: &[LogEvent], lines: &[String]) -> Result<String, String> {
    let Some(transcript) = build_transcript(events, lines) else {
        return Err("nothing to name yet".to_string());
    };
    let prompt = format!("{PROMPT_PREAMBLE}{transcript}");
    let name = ask(&prompt, RENAME_TIMEOUT).await?;
    let name = name.trim().trim_matches(['"', '\'']).replace('\n', " ");
    if name.is_empty() {
        return Err("empty name".to_string());
    }
    Ok(truncate(&name, MAX_NAME_CHARS))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}
