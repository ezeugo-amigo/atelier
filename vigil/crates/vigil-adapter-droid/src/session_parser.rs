use std::collections::HashMap;
use vigil_core::{LogEvent, ToolKind};

/// Parse a Droid (Factory) JSONL session file into structured log events.
///
/// The droid JSONL format:
/// - First line: `{"type":"session_start","id":"<uuid>","cwd":"<path>",...}`
/// - Subsequent lines: `{"type":"message","timestamp":"...","message":{"role":"user"|"assistant","content":[...]},...}`
/// - Lines with `"visibility":"llm_only"` are context injections — skip them.
/// - User messages whose content consists entirely of `tool_result` blocks are
///   system-injected tool responses — skip them as standalone turns.
pub fn parse_session_jsonl(content: &str) -> Vec<LogEvent> {
    let mut events: Vec<LogEvent> = Vec::new();
    let mut pending_user: Option<(String, Option<String>)> = None;
    let mut pending_tools: HashMap<String, u32> = HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        if val["type"].as_str() != Some("message") {
            continue;
        }
        // Skip system context injections.
        if val["visibility"].as_str() == Some("llm_only") {
            continue;
        }

        let ts = val["timestamp"]
            .as_str()
            .and_then(|s| s.get(11..16))
            .map(str::to_string);

        match val["message"]["role"].as_str().unwrap_or("") {
            "user" => {
                let content_val = &val["message"]["content"];
                // Skip pure tool-result messages (system-injected, not user turns).
                let is_only_tool_results = content_val
                    .as_array()
                    .map(|arr| {
                        !arr.is_empty()
                            && arr
                                .iter()
                                .all(|b| b["type"].as_str() == Some("tool_result"))
                    })
                    .unwrap_or(false);
                if is_only_tool_results {
                    continue;
                }

                flush_pending_turn(&mut events, &mut pending_user, &mut pending_tools);
                if let Some(text) = extract_user_text(content_val) {
                    pending_user = Some((text, ts));
                }
            }
            "assistant" => {
                let Some(content_arr) = val["message"]["content"].as_array() else {
                    continue;
                };

                let mut text_parts: Vec<String> = Vec::new();
                for block in content_arr {
                    match block["type"].as_str().unwrap_or("") {
                        "text" => {
                            if let Some(t) = block["text"].as_str() {
                                let t = t.trim();
                                if !t.is_empty() {
                                    text_parts.push(t.to_string());
                                }
                            }
                        }
                        "tool_use" => {
                            let name = block["name"].as_str().unwrap_or("unknown").to_string();
                            *pending_tools.entry(name).or_insert(0) += 1;
                        }
                        // Skip "thinking" and other block types.
                        _ => {}
                    }
                }

                if !text_parts.is_empty() {
                    if let Some((user_text, user_time)) = pending_user.take() {
                        events.push(LogEvent::UserMessage {
                            text: user_text,
                            time: user_time,
                        });
                    }
                    emit_tool_group(&mut events, &mut pending_tools);
                    events.push(LogEvent::AgentMessage {
                        text: text_parts.join("\n\n"),
                        time: ts,
                        label: "DR".to_string(),
                    });
                }
                // Tool-only assistant messages: tools accumulate for the next text response.
            }
            _ => {}
        }
    }

    // Flush any in-progress turn (agent still working).
    flush_pending_turn(&mut events, &mut pending_user, &mut pending_tools);
    events
}

fn extract_user_text(content: &serde_json::Value) -> Option<String> {
    match content {
        serde_json::Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                None
            } else {
                Some(s.chars().take(2000).collect())
            }
        }
        serde_json::Value::Array(arr) => {
            let text: String = arr
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .filter(|s| !s.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            if text.is_empty() {
                None
            } else {
                Some(text.chars().take(2000).collect())
            }
        }
        _ => None,
    }
}

fn flush_pending_turn(
    events: &mut Vec<LogEvent>,
    pending_user: &mut Option<(String, Option<String>)>,
    pending_tools: &mut HashMap<String, u32>,
) {
    if let Some((text, time)) = pending_user.take() {
        events.push(LogEvent::UserMessage { text, time });
    }
    emit_tool_group(events, pending_tools);
}

fn emit_tool_group(events: &mut Vec<LogEvent>, pending_tools: &mut HashMap<String, u32>) {
    if pending_tools.is_empty() {
        return;
    }
    let mut kind_map: HashMap<ToolKind, u32> = HashMap::new();
    for (name, count) in pending_tools.drain() {
        let kind = ToolKind::from_name(&name);
        *kind_map.entry(kind).or_insert(0) += count;
    }
    let mut tools: Vec<(ToolKind, u32)> = kind_map.into_iter().collect();
    tools.sort_by_key(|(k, _)| match k {
        ToolKind::Read => 0,
        ToolKind::Bash => 1,
        ToolKind::Edit => 2,
        ToolKind::Other(_) => 3,
    });
    events.push(LogEvent::ToolGroup { tools });
}
