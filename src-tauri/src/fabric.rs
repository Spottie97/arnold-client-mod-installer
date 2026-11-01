use crate::errors::AppError;
use crate::manifest::Loader;
use std::path::Path;

/// Phase 1 stub. Writes the Fabric version JSON and launcher profile in a later milestone.
pub fn install_loader(_game_dir: &Path, _loader: &Loader) -> Result<(), AppError> {
    Err(AppError::NotImplemented {
        what: "Fabric loader install".into(),
    })
}
