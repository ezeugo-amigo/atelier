use std::path::PathBuf;
use crate::{AgentKind, FsSignals, Session, SessionId, SessionLog, SessionState, VigilError};

#[async_trait::async_trait]
pub trait AgentAdapter: Send + Sync {
    fn kind(&self) -> AgentKind;
    fn roots(&self) -> Vec<PathBuf>;
    async fn discover(&self) -> Result<Vec<SessionId>, VigilError>;
    async fn read(&self, id: &SessionId) -> Result<Session, VigilError>;
    fn classify(&self, log: &SessionLog, fs: &FsSignals) -> SessionState;
    fn attach_command(&self, id: &SessionId) -> std::process::Command;
}
