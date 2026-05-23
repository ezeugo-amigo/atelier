use std::path::PathBuf;
use vigil_adapter_claude::discover::discover_sessions;

#[tokio::test]
async fn discover_from_real_debug_dir() {
    let home = std::env::var("HOME").expect("HOME must be set");
    let debug_dir = PathBuf::from(home).join(".claude").join("debug");

    if !debug_dir.exists() {
        eprintln!("Skipping: ~/.claude/debug does not exist");
        return;
    }

    let ids = discover_sessions(&debug_dir)
        .await
        .expect("discover should succeed");
    // Should find at least one UUID session file (if any exist)
    println!("Found {} sessions", ids.len());
    for id in ids.iter().take(5) {
        println!("  {}", id);
        // Validate UUID format
        let parts: Vec<&str> = id.0.split('-').collect();
        assert_eq!(parts.len(), 5, "session ID should be UUID format: {}", id);
    }
}
