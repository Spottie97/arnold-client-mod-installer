use crate::errors::AppError;
use std::path::{Path, PathBuf};

/// Phase 1 stub. JSONL export is a later milestone.
pub fn export_log(_game_dir: Option<&Path>) -> Result<PathBuf, AppError> {
    Err(AppError::NotImplemented {
        what: "log export".into(),
    })
}
