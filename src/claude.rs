use std::path::{Path, PathBuf};

pub fn log_path(log_dir: &Path, command: &str, task_id: &str) -> PathBuf {
    crate::backend::log_path(log_dir, command, task_id)
}
