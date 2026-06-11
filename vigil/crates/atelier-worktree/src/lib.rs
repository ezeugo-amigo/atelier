pub mod create;
pub mod names;
pub mod registry;
pub mod remove;

pub use create::{create, resolve_repo_root, workspaces_root, CreateOptions};
pub use names::generate_name;
pub use registry::{Registry, RepoCheckout, WorktreeEntry, WorktreeError};
pub use remove::{prune, remove, RemoveOptions};
