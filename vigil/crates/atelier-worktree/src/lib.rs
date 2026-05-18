pub mod create;
pub mod names;
pub mod registry;
pub mod remove;

pub use create::{CreateOptions, create, resolve_repo_root};
pub use names::generate_name;
pub use registry::{Registry, WorktreeEntry, WorktreeError};
pub use remove::{RemoveOptions, prune, remove};
