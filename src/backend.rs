use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result};
use tracing::{info, warn};

use crate::activity;

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

pub struct RunParams<'a> {
    pub wt_path: &'a Path,
    pub prompt: &'a str,
    pub log_file: &'a Path,
    pub command_name: &'a str,
    pub project: &'a str,
    pub task_id: &'a str,
    pub model: &'a str,
}

pub trait AiBackend {
    fn build_command(&self, params: &RunParams) -> Result<Command>;

    fn run(&self, params: &RunParams) -> Result<bool> {
        if let Some(dir) = params.log_file.parent() {
            fs::create_dir_all(dir).ok();
        }
        info!("Log: {}", params.log_file.display());

        let started_at = chrono::Local::now();
        let timer = Instant::now();

        let output_file = fs::File::create(params.log_file).context("Failed to create log file")?;
        let stderr_file = output_file.try_clone()?;

        let mut cmd = self.build_command(params)?;
        let status = cmd
            .current_dir(params.wt_path)
            .stdout(output_file)
            .stderr(stderr_file)
            .status()
            .context("Failed to run AI backend")?;

        let elapsed = timer.elapsed();
        let finished_at = chrono::Local::now();
        let success = status.success();

        let log_dir = params.log_file.parent().unwrap_or_else(|| Path::new("."));
        activity::record(
            log_dir,
            &activity::Entry {
                command: params.command_name.to_string(),
                project: params.project.to_string(),
                task_id: params.task_id.to_string(),
                started_at: started_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
                finished_at: finished_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
                duration_secs: elapsed.as_secs(),
                success,
            },
        );

        Ok(success)
    }
}

pub struct ClaudeBackend {
    pub max_budget: Option<u32>,
}

impl AiBackend for ClaudeBackend {
    fn build_command(&self, params: &RunParams) -> Result<Command> {
        if let Some(b) = self.max_budget {
            info!("Running Claude (model={}, budget=${b})...", params.model);
        } else {
            info!(
                "Running Claude (model={}, budget=unlimited)...",
                params.model
            );
        }

        // `--dangerously-skip-permissions` is required for unattended operation: Claude cannot
        // prompt for permission approvals when running non-interactively. This means the spawned
        // process has unrestricted filesystem and command execution access. Task descriptions
        // become untrusted code execution vectors — see the "Security / Trust Model" section in
        // CLAUDE.md for the full trust boundary analysis and operator guidance.
        let mut args = vec![
            "-p".to_string(),
            "--dangerously-skip-permissions".to_string(),
            "--model".to_string(),
            params.model.to_string(),
        ];
        if let Some(b) = self.max_budget {
            args.push("--max-budget-usd".to_string());
            args.push(b.to_string());
        }
        args.push(params.prompt.to_string());

        let mut cmd = Command::new("claude");
        cmd.args(&args);
        Ok(cmd)
    }
}

pub struct CodexBackend {
    pub max_budget: Option<u32>,
    pub reasoning_effort: String,
}

impl AiBackend for CodexBackend {
    fn build_command(&self, params: &RunParams) -> Result<Command> {
        if self.max_budget.is_some() {
            warn!("max_budget is configured but codex does not support --max-budget-usd; ignoring");
        }

        info!(
            "Running Codex (model={}, reasoning_effort={})...",
            params.model, self.reasoning_effort
        );

        // `--full-auto` is required for unattended operation: Codex cannot prompt for approval
        // when running non-interactively. This grants unrestricted execution access.
        // See the "Security / Trust Model" section in CLAUDE.md for the full trust boundary analysis.
        let effort_config = format!("model_reasoning_effort=\"{}\"", self.reasoning_effort);
        let mut cmd = Command::new("codex");
        cmd.args([
            "exec",
            "--full-auto",
            "--model",
            params.model,
            "--config",
            &effort_config,
            params.prompt,
        ]);
        Ok(cmd)
    }
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
