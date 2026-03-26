use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

const FILENAME: &str = "activity.jsonl";

#[derive(Serialize, Deserialize)]
pub struct Entry {
    pub command: String,
    pub project: String,
    pub task_id: String,
    pub started_at: String,
    pub finished_at: String,
    pub duration_secs: u64,
    pub success: bool,
}

pub fn record(log_dir: &Path, entry: &Entry) {
    fs::create_dir_all(log_dir).ok();
    let path = log_dir.join(FILENAME);
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let Ok(json) = serde_json::to_string(entry) else {
        return;
    };
    writeln!(file, "{json}").ok();
}

pub fn read_recent(log_dir: &Path, limit: usize) -> Vec<Entry> {
    let path = log_dir.join(FILENAME);
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };

    let mut entries: Vec<Entry> = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect();

    // Most recent last in file, so reverse and take limit
    entries.reverse();
    entries.truncate(limit);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = std::path::PathBuf::from(format!(
            "{}/nocturnal-activity-test-{}",
            std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string()),
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let entry = Entry {
            command: "implement".to_string(),
            project: "myproj".to_string(),
            task_id: "td-abc".to_string(),
            started_at: "2026-03-16T14:30:00".to_string(),
            finished_at: "2026-03-16T14:35:23".to_string(),
            duration_secs: 323,
            success: true,
        };
        record(&dir, &entry);
        record(
            &dir,
            &Entry {
                command: "review".to_string(),
                project: "other".to_string(),
                task_id: "td-def".to_string(),
                started_at: "2026-03-16T15:00:00".to_string(),
                finished_at: "2026-03-16T15:02:10".to_string(),
                duration_secs: 130,
                success: false,
            },
        );

        let entries = read_recent(&dir, 5);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].command, "review");
        assert_eq!(entries[1].command, "implement");

        let entries = read_recent(&dir, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].task_id, "td-def");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_empty_dir() {
        let entries = read_recent(std::path::Path::new("/nonexistent/path"), 5);
        assert!(entries.is_empty());
    }
}
