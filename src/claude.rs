use std::path::{Path, PathBuf};

pub fn log_path(log_dir: &Path, command: &str, task_id: &str) -> PathBuf {
    debug_assert!(
        crate::td::validate_task_id(task_id).is_ok(),
        "task_id must be validated before constructing log path: {task_id:?}"
    );
    log_dir.join(format!(
        "{}-{}-{}.log",
        command,
        task_id,
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_path_contains_all_components() {
        let path = log_path(Path::new("/tmp/logs"), "implement", "task-42");
        let s = path.to_string_lossy();
        assert!(s.starts_with("/tmp/logs/implement-task-42-"));
        assert!(s.ends_with(".log"));
    }

    #[test]
    fn log_path_format_has_timestamp() {
        let path = log_path(Path::new("/logs"), "review", "t1");
        let s = path.to_string_lossy();
        // Format: /logs/review-t1-YYYYMMDD-HHMMSS.log
        let suffix = s.strip_prefix("/logs/review-t1-").unwrap();
        let timestamp = suffix.strip_suffix(".log").unwrap();
        // Should be like "20260316-143052"
        assert_eq!(timestamp.len(), 15);
        assert_eq!(timestamp.as_bytes()[8], b'-');
    }
}
