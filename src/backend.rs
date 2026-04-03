use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result};
use tracing::info;

use crate::activity;

pub trait AiBackend {
    #[allow(clippy::too_many_arguments)]
    fn run(
        &self,
        wt_path: &Path,
        prompt: &str,
        log_file: &Path,
        command_name: &str,
        project: &str,
        task_id: &str,
        model: &str,
    ) -> Result<bool>;
}

pub struct ClaudeBackend {
    pub log_dir: PathBuf,
    pub max_budget: Option<u32>,
}

impl AiBackend for ClaudeBackend {
    #[allow(clippy::too_many_arguments)]
    fn run(
        &self,
        wt_path: &Path,
        prompt: &str,
        log_file: &Path,
        command_name: &str,
        project: &str,
        task_id: &str,
        model: &str,
    ) -> Result<bool> {
        fs::create_dir_all(&self.log_dir).ok();

        match self.max_budget {
            Some(b) => info!("Running Claude (model={model}, budget=${b})..."),
            None => info!("Running Claude (model={model}, budget=unlimited)..."),
        }
        info!("Log: {}", log_file.display());

        let started_at = chrono::Local::now();
        let timer = Instant::now();

        let output_file = fs::File::create(log_file).context("Failed to create log file")?;
        let stderr_file = output_file.try_clone()?;

        // `--dangerously-skip-permissions` is required for unattended operation: Claude cannot
        // prompt for permission approvals when running non-interactively. This means the spawned
        // process has unrestricted filesystem and command execution access. Task descriptions
        // become untrusted code execution vectors — see the "Security / Trust Model" section in
        // CLAUDE.md for the full trust boundary analysis and operator guidance.
        let mut args = vec!["-p", "--dangerously-skip-permissions", "--model", model];
        let budget_str;
        if let Some(b) = self.max_budget {
            budget_str = b.to_string();
            args.push("--max-budget-usd");
            args.push(&budget_str);
        }
        args.push(prompt);

        let status = Command::new("claude")
            .args(&args)
            .current_dir(wt_path)
            .stdout(output_file)
            .stderr(stderr_file)
            .status()
            .context("Failed to run claude")?;

        let elapsed = timer.elapsed();
        let finished_at = chrono::Local::now();
        let success = status.success();

        activity::record(
            &self.log_dir,
            &activity::Entry {
                command: command_name.to_string(),
                project: project.to_string(),
                task_id: task_id.to_string(),
                started_at: started_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
                finished_at: finished_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
                duration_secs: elapsed.as_secs(),
                success,
            },
        );

        Ok(success)
    }
}
