use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::{Deserialize, Serialize};

const FILENAME: &str = "activity.jsonl";
const CAPACITY: usize = 10;

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
    let Ok(json) = serde_json::to_string(entry) else {
        return;
    };

    let mut lines: Vec<String> = fs::File::open(&path)
        .ok()
        .map(|f| {
            BufReader::new(f)
                .lines()
                .map_while(Result::ok)
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default();
    lines.push(json);

    let start = lines.len().saturating_sub(CAPACITY);
    fs::write(path, lines[start..].join("\n") + "\n").ok();
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

    #[test]
    fn capacity_is_respected() {
        let dir = std::path::PathBuf::from(format!(
            "{}/nocturnal-activity-cap-{}",
            std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string()),
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        for i in 0..15 {
            record(
                &dir,
                &Entry {
                    command: format!("cmd-{i}"),
                    project: "p".to_string(),
                    task_id: format!("t-{i}"),
                    started_at: String::new(),
                    finished_at: String::new(),
                    duration_secs: i as u64,
                    success: true,
                },
            );
        }

        // File should contain at most CAPACITY entries
        let all = read_recent(&dir, CAPACITY);
        assert_eq!(all.len(), CAPACITY);
        // Most recent first
        assert_eq!(all[0].command, "cmd-14");
        assert_eq!(all[CAPACITY - 1].command, "cmd-5");

        let _ = fs::remove_dir_all(&dir);
    }
}
