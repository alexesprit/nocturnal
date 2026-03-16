use std::fs;
use std::process::Command;

use anyhow::{Context, Result};
use tracing::info;

use crate::config::ProjectContext;

pub fn log_path(log_dir: &str, command: &str, task_id: &str) -> String {
    format!(
        "{}/{}-{}-{}.log",
        log_dir,
        command,
        task_id,
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    )
}

pub fn run(ctx: &ProjectContext, wt_path: &str, prompt: &str, log_file: &str) -> Result<bool> {
    fs::create_dir_all(&ctx.cfg.log_dir).ok();

    info!(
        "Running Claude (model={}, budget=${})...",
        ctx.cfg.model, ctx.cfg.max_budget
    );
    info!("Log: {log_file}");

    let output_file = fs::File::create(log_file).context("Failed to create log file")?;
    let stderr_file = output_file.try_clone()?;

    let status = Command::new("claude")
        .args([
            "-p",
            "--dangerously-skip-permissions",
            "--model",
            &ctx.cfg.model,
            "--max-budget-usd",
            &ctx.cfg.max_budget.to_string(),
            prompt,
        ])
        .current_dir(wt_path)
        .stdout(output_file)
        .stderr(stderr_file)
        .status()
        .context("Failed to run claude")?;

    Ok(status.success())
}
