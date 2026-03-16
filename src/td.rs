use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer};
use tracing::debug;

fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::deserialize(deserializer)?.unwrap_or_default())
}

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
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
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
        let json = self.run(&["list", "--json", "--status", "open", "--limit", "1"])?;
        let tasks: Vec<Task> = serde_json::from_str(&json).unwrap_or_default();
        Ok(tasks.into_iter().next().map(|t| t.id))
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

    pub fn start(&self, task_id: &str) -> Result<()> {
        self.run_quiet(&["start", task_id])
    }

    pub fn review(&self, task_id: &str) -> Result<()> {
        self.run_quiet(&["review", task_id])
    }

    pub fn approve(&self, task_id: &str) -> Result<()> {
        self.run_quiet(&["approve", task_id])
    }

    pub fn reject(&self, task_id: &str, reason: &str) -> Result<()> {
        self.run_quiet(&["reject", task_id, "--reason", reason])
    }

    pub fn block(&self, task_id: &str) -> Result<()> {
        self.run_quiet(&["block", task_id])
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
    let mut labels: Vec<String> = task
        .labels
        .iter()
        .filter(|l| !l.starts_with(remove_prefix))
        .cloned()
        .collect();
    if let Some(add) = add_label {
        labels.push(add.to_string());
    }
    labels.join(",")
}
