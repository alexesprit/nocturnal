use std::fs;
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result};
use tracing::info;

use crate::activity;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_path_contains_all_components() {
        let path = log_path("/tmp/logs", "implement", "task-42");
        assert!(path.starts_with("/tmp/logs/implement-task-42-"));
        assert!(path.ends_with(".log"));
    }

    #[test]
    fn log_path_format_has_timestamp() {
        let path = log_path("/logs", "review", "t1");
        // Format: /logs/review-t1-YYYYMMDD-HHMMSS.log
        let suffix = path.strip_prefix("/logs/review-t1-").unwrap();
        let timestamp = suffix.strip_suffix(".log").unwrap();
        // Should be like "20260316-143052"
        assert_eq!(timestamp.len(), 15);
        assert_eq!(timestamp.as_bytes()[8], b'-');
    }
}

pub fn run(
    ctx: &ProjectContext,
    wt_path: &str,
    prompt: &str,
    log_file: &str,
    command_name: &str,
    project: &str,
    task_id: &str,
) -> Result<bool> {
    fs::create_dir_all(&ctx.cfg.log_dir).ok();

    match ctx.cfg.max_budget {
        Some(b) => info!("Running Claude (model={}, budget=${b})...", ctx.cfg.model),
        None => info!(
            "Running Claude (model={}, budget=unlimited)...",
            ctx.cfg.model
        ),
    }
    info!("Log: {log_file}");

    let started_at = chrono::Local::now();
    let timer = Instant::now();

    let output_file = fs::File::create(log_file).context("Failed to create log file")?;
    let stderr_file = output_file.try_clone()?;

    let mut args = vec![
        "-p",
        "--dangerously-skip-permissions",
        "--model",
        &ctx.cfg.model,
    ];
    let budget_str;
    if let Some(b) = ctx.cfg.max_budget {
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
        &ctx.cfg.log_dir,
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
