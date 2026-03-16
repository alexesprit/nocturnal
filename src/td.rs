use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer};
use tracing::debug;

pub(crate) fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::deserialize(deserializer)?.unwrap_or_default())
}

#[allow(dead_code)] // fields used by askama templates
#[derive(Debug, Deserialize)]
pub struct Task {
    pub id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub title: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub description: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub status: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub labels: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub priority: String,
    #[serde(rename = "type", default, deserialize_with = "null_as_default")]
    pub task_type: String,
    #[serde(default)]
    pub points: Option<i32>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub sprint: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub created_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub updated_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub closed_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub parent_id: String,
}

pub struct Td<'a> {
    project_root: &'a str,
}

impl<'a> Td<'a> {
    pub fn new(project_root: &'a str) -> Self {
        Self { project_root }
    }

    fn cmd(&self) -> Command {
        let mut cmd = Command::new("td");
        cmd.args(["-w", self.project_root]);
        cmd
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let output = self.cmd().args(args).output().context("Failed to run td")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("td {} failed: {}", args.join(" "), stderr.trim());
        }
        String::from_utf8(output.stdout).context("td output was not valid UTF-8")
    }

    fn run_quiet(&self, args: &[&str]) -> Result<()> {
        if let Err(e) = self.run(args) {
            debug!("td {} (ignored): {e:#}", args.join(" "));
        }
        Ok(())
    }

    pub fn show(&self, task_id: &str) -> Result<Task> {
        let json = self.run(&["show", task_id, "--json"])?;
        serde_json::from_str(&json).context("Failed to parse task JSON")
    }

    pub fn list_by_status(&self, status: &str) -> Result<Vec<Task>> {
        let json = self.run(&["list", "--json", "--status", status])?;
        Ok(serde_json::from_str(&json).unwrap_or_default())
    }

    pub fn get_next_task_id(&self) -> Result<Option<String>> {
        let output = self.run(&["next"]);
        match output {
            Ok(stdout) => Ok(stdout.split_whitespace().next().map(|s| s.to_string())),
            Err(_) => Ok(None),
        }
    }

    pub fn get_reviewable_task_id(&self) -> Result<Option<String>> {
        let tasks = self.list_by_status("in_review")?;
        Ok(tasks
            .into_iter()
            .find(|t| !t.labels.iter().any(|l| l.starts_with("noc-proposal")))
            .map(|t| t.id))
    }

    pub fn get_proposal_task_ids(&self) -> Result<Vec<String>> {
        let tasks = self.list_by_status("in_review")?;
        Ok(tasks
            .into_iter()
            .filter(|t| t.labels.iter().any(|l| l.starts_with("noc-proposal:")))
            .map(|t| t.id)
            .collect())
    }

    /// Determine the next action the orchestrator would take.
    /// `check_proposals` should be true when VCS integration is active.
    pub fn get_next_action(&self, check_proposals: bool) -> Result<NextAction> {
        if check_proposals {
            let proposals = self.get_proposal_task_ids()?;
            if !proposals.is_empty() {
                return Ok(NextAction::ProposalReview(proposals));
            }
        }

        if let Some(task_id) = self.get_reviewable_task_id()? {
            return Ok(NextAction::Review(task_id));
        }

        if let Some(task_id) = self.get_next_task_id()? {
            return Ok(NextAction::Implement(task_id));
        }

        Ok(NextAction::Idle)
    }

    pub fn start(&self, task_id: &str) -> Result<()> {
        self.run_quiet(&["start", task_id])
    }

    pub fn review(&self, task_id: &str) -> Result<()> {
        self.run_quiet(&["review", task_id])
    }

    pub fn approve(&self, task_id: &str) -> Result<()> {
        self.run(&["approve", task_id]).map(|_| ())
    }

    pub fn reject(&self, task_id: &str, reason: &str) -> Result<()> {
        self.run(&["reject", task_id, "--reason", reason])
            .map(|_| ())
    }

    pub fn block(&self, task_id: &str) -> Result<()> {
        self.run(&["block", task_id]).map(|_| ())
    }

    pub fn comment(&self, task_id: &str, message: &str) -> Result<()> {
        self.run_quiet(&["comment", task_id, message])
    }

    pub fn log(&self, message: &str) -> Result<()> {
        self.run_quiet(&["log", message])
    }

    pub fn update_labels(&self, task_id: &str, labels: &str) -> Result<()> {
        self.run(&["update", task_id, "--labels", labels])?;
        Ok(())
    }
}

pub enum NextAction {
    ProposalReview(Vec<String>),
    Review(String),
    Implement(String),
    Idle,
}

impl NextAction {
    pub fn task_id(&self) -> Option<&str> {
        match self {
            NextAction::ProposalReview(ids) => ids.first().map(|s| s.as_str()),
            NextAction::Review(id) | NextAction::Implement(id) => Some(id),
            NextAction::Idle => None,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            NextAction::ProposalReview(_) => "Proposal Review",
            NextAction::Review(_) => "Review",
            NextAction::Implement(_) => "Implement",
            NextAction::Idle => "Idle",
        }
    }
}

// --- Label helpers ---

pub fn get_review_count(task: &Task) -> u32 {
    task.labels
        .iter()
        .find_map(|l| l.strip_prefix("noc-reviews:"))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

pub fn build_labels_with_review_count(task: &Task, count: u32) -> String {
    let mut labels: Vec<String> = task
        .labels
        .iter()
        .filter(|l| !l.starts_with("noc-reviews:"))
        .cloned()
        .collect();
    labels.push(format!("noc-reviews:{count}"));
    labels.join(",")
}

pub fn swap_label(task: &Task, remove_prefix: &str, add_label: Option<&str>) -> String {
    swap_labels(task, &[remove_prefix], add_label)
}

pub fn swap_labels(task: &Task, remove_prefixes: &[&str], add_label: Option<&str>) -> String {
    let mut labels: Vec<String> = task
        .labels
        .iter()
        .filter(|l| !remove_prefixes.iter().any(|p| l.starts_with(p)))
        .cloned()
        .collect();
    if let Some(add) = add_label {
        labels.push(add.to_string());
    }
    labels.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task_with_labels(labels: &[&str]) -> Task {
        Task {
            id: "t1".to_string(),
            title: String::new(),
            description: String::new(),
            status: String::new(),
            labels: labels.iter().map(|s| s.to_string()).collect(),
            priority: String::new(),
            task_type: String::new(),
            points: None,
            sprint: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            closed_at: String::new(),
            parent_id: String::new(),
        }
    }

    // --- get_review_count ---

    #[test]
    fn get_review_count_returns_zero_when_no_label() {
        let task = task_with_labels(&["bug", "urgent"]);
        assert_eq!(get_review_count(&task), 0);
    }

    #[test]
    fn get_review_count_returns_zero_for_empty_labels() {
        let task = task_with_labels(&[]);
        assert_eq!(get_review_count(&task), 0);
    }

    #[test]
    fn get_review_count_parses_label() {
        let task = task_with_labels(&["noc-reviews:2", "bug"]);
        assert_eq!(get_review_count(&task), 2);
    }

    #[test]
    fn get_review_count_returns_zero_for_malformed_value() {
        let task = task_with_labels(&["noc-reviews:abc"]);
        assert_eq!(get_review_count(&task), 0);
    }

    #[test]
    fn get_review_count_returns_zero_for_empty_value() {
        let task = task_with_labels(&["noc-reviews:"]);
        assert_eq!(get_review_count(&task), 0);
    }

    // --- build_labels_with_review_count ---

    #[test]
    fn build_labels_replaces_existing_review_count() {
        let task = task_with_labels(&["bug", "noc-reviews:1"]);
        assert_eq!(
            build_labels_with_review_count(&task, 2),
            "bug,noc-reviews:2"
        );
    }

    #[test]
    fn build_labels_adds_count_when_missing() {
        let task = task_with_labels(&["bug"]);
        assert_eq!(
            build_labels_with_review_count(&task, 1),
            "bug,noc-reviews:1"
        );
    }

    #[test]
    fn build_labels_with_empty_labels() {
        let task = task_with_labels(&[]);
        assert_eq!(build_labels_with_review_count(&task, 0), "noc-reviews:0");
    }

    // --- swap_label ---

    #[test]
    fn swap_label_removes_prefix_and_adds_new() {
        let task = task_with_labels(&["noc-proposal:42", "bug"]);
        assert_eq!(
            swap_label(&task, "noc-proposal", Some("noc-done")),
            "bug,noc-done"
        );
    }

    #[test]
    fn swap_label_removes_without_adding() {
        let task = task_with_labels(&["noc-proposal:42", "bug"]);
        assert_eq!(swap_label(&task, "noc-proposal", None), "bug");
    }

    #[test]
    fn swap_label_no_match_keeps_all_and_adds() {
        let task = task_with_labels(&["bug", "urgent"]);
        assert_eq!(
            swap_label(&task, "noc-proposal", Some("new")),
            "bug,urgent,new"
        );
    }

    // --- swap_labels (multiple prefixes) ---

    #[test]
    fn swap_labels_removes_multiple_prefixes() {
        let task = task_with_labels(&["noc-proposal:42", "noc-proposal-ready", "bug"]);
        assert_eq!(
            swap_labels(
                &task,
                &["noc-proposal:", "noc-proposal-ready"],
                Some("done")
            ),
            "bug,done"
        );
    }

    // --- Task deserialization ---

    #[test]
    fn task_deserializes_with_null_fields() {
        let json =
            r#"{"id": "t1", "title": null, "description": null, "status": null, "labels": null}"#;
        let task: Task = serde_json::from_str(json).unwrap();
        assert_eq!(task.id, "t1");
        assert_eq!(task.title, "");
        assert_eq!(task.description, "");
        assert_eq!(task.status, "");
        assert!(task.labels.is_empty());
    }

    #[test]
    fn task_deserializes_with_missing_fields() {
        let json = r#"{"id": "t1"}"#;
        let task: Task = serde_json::from_str(json).unwrap();
        assert_eq!(task.id, "t1");
        assert!(task.labels.is_empty());
    }

    #[test]
    fn task_deserializes_with_full_fields() {
        let json = r#"{"id": "t1", "title": "Fix bug", "description": "desc", "status": "open", "labels": ["a", "b"]}"#;
        let task: Task = serde_json::from_str(json).unwrap();
        assert_eq!(task.title, "Fix bug");
        assert_eq!(task.labels, vec!["a", "b"]);
    }
}
