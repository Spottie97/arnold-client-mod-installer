use crate::errors::AppError;
use crate::plan::InstallPlan;
use std::path::Path;

/// Phase 1 stub. The transaction (verify → snapshot → stage → rename) lands in M1.
pub fn run_install(_game_dir: &Path, _plan: &InstallPlan) -> Result<String, AppError> {
    Err(AppError::NotImplemented {
        what: "install transaction".into(),
    })
}

pub fn cancel_install(_run_id: &str) -> Result<(), AppError> {
    Err(AppError::NotImplemented {
        what: "install cancel".into(),
    })
}
