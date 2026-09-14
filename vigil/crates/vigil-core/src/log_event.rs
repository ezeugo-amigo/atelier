/// Structured log event for the timeline log-view overlay.

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ToolKind {
    Read,
    Bash,
    Edit,
    Other(String),
}

impl ToolKind {
    pub fn from_name(name: &str) -> Self {
        let lower = name.to_lowercase();
        if lower.contains("read") || lower.contains("glob") || lower.contains("grep") {
            ToolKind::Read
        } else if lower.contains("bash") || lower.contains("shell") || lower == "run" {
            ToolKind::Bash
        } else if lower.contains("edit") || lower.contains("write") {
            ToolKind::Edit
        } else {
            ToolKind::Other(name.to_string())
        }
    }

    pub fn label(&self) -> &str {
        match self {
            ToolKind::Read => "read",
            ToolKind::Bash => "bash",
            ToolKind::Edit => "edit",
            ToolKind::Other(s) => s.as_str(),
        }
    }
}

/// A single tool invocation, retained for the expanded ("t"-toggled) log view.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub kind: ToolKind,
    /// The tool's raw name as reported by the agent (e.g. "Bash", "Read").
    pub name: String,
    /// Best-effort summary of the call's input (e.g. the bash command or file path).
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
pub enum LogEvent {
    UserMessage {
        text: String,
        time: Option<String>,
    },
    AgentMessage {
        text: String,
        time: Option<String>,
        label: String,
    },
    ToolGroup {
        /// Tool calls condensed between turns: kind → count. Used for the collapsed bar view.
        tools: Vec<(ToolKind, u32)>,
        /// The individual calls in original order, for the expanded view.
        calls: Vec<ToolCall>,
    },
}

/// Best-effort summary of a tool call's input/arguments object, for display in the
/// expanded log view (e.g. the bash command, or the file path for a read/edit).
pub fn summarize_tool_input(input: &serde_json::Value) -> Option<String> {
    let obj = input.as_object()?;
    let value = [
        "command",
        "file_path",
        "path",
        "pattern",
        "url",
        "query",
        "prompt",
        "description",
    ]
    .iter()
    .find_map(|key| obj.get(*key).and_then(|v| v.as_str()))
    .or_else(|| obj.values().find_map(|v| v.as_str()))?;
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed.chars().take(200).collect())
    }
}
