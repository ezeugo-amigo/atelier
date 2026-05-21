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

#[derive(Debug, Clone)]
pub enum LogEvent {
    UserMessage { text: String, time: Option<String> },
    AgentMessage { text: String, time: Option<String>, label: String },
    /// Tool calls condensed between turns: kind → count.
    ToolGroup { tools: Vec<(ToolKind, u32)> },
}
