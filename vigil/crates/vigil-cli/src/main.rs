use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use vigil_core::{AgentAdapter, AgentKind};
use vigil_adapter_claude::ClaudeCodeAdapter;
use vigil_adapter_codex::CodexAdapter;
use vigil_adapter_pi::PiAdapter;
use vigil_adapter_opencode::OpenCodeAdapter;

#[tokio::main]
async fn main() -> Result<()> {
    let mut adapters: HashMap<AgentKind, Arc<dyn AgentAdapter>> = HashMap::new();
    adapters.insert(AgentKind::ClaudeCode, Arc::new(ClaudeCodeAdapter::new()?));
    adapters.insert(AgentKind::Codex,      Arc::new(CodexAdapter));
    adapters.insert(AgentKind::Pi,         Arc::new(PiAdapter));
    adapters.insert(AgentKind::OpenCode,   Arc::new(OpenCodeAdapter));
    vigil_tui::run(adapters).await
}
