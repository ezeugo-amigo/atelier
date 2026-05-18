use std::sync::Arc;

use anyhow::Result;
use vigil_adapter_claude::ClaudeCodeAdapter;

#[tokio::main]
async fn main() -> Result<()> {
    let adapter = Arc::new(ClaudeCodeAdapter::new()?);
    vigil_tui::run(adapter).await
}
