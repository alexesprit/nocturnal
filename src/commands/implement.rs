use anyhow::Result;
use tracing::{error, info, warn};

use crate::config::ProjectContext;
use crate::{backend, git, preflight, prompt, td};
/// Implement a specific task. Returns Ok(true) if implementation succeeded
/// and the task moved to review.
pub fn implement_task(ctx: &ProjectContext, task_id: &str) -> Result<bool> {
    let td = td::Td::new(&ctx.project_root);

    info!("=== Implementing task: {task_id} ===");

    let task = td.show(task_id)?;
    let review_count = td::get_review_count(&task);
    if review_count >= ctx.settings.max_reviews {
        info!(
            "Task {task_id} has {review_count} review cycles (max {}), skipping",
            ctx.settings.max_reviews
        );
        return Ok(false);
    }

    if ctx.cfg.dry_run {
        info!("dry-run: would create worktree for task {task_id}");
        info!("dry-run: would invoke Claude for implement of task {task_id}");
        info!("dry-run: would submit task {task_id} for review");
        return Ok(false);
    }

    preflight::run_checks(ctx)?;

    let local_only = ctx.settings.vcs_mode == crate::project_config::VcsMode::Local;
    let wt_path = git::ensure_worktree(
        &ctx.project_root,
        task_id,
        &ctx.settings.base_branch,
        local_only,
    )?;
    info!("Worktree: {}", wt_path.display());

    // best-effort: task may already be in_progress from a previous attempt
    td.start(task_id).ok();

    let rendered = prompt::render_base(
        prompt::Template::Implement,
        task_id,
        &ctx.project_root,
        ctx.settings.max_reviews,
        &ctx.settings.base_branch,
    );

    let slug = ctx.project_slug();
    let log_file = backend::log_path(&ctx.cfg.log_dir, "implement", task_id);

    if ctx.implement_backend.run(&backend::RunParams {
        wt_path: &wt_path,
        prompt: &rendered,
        log_file: &log_file,
        command_name: "implement",
        project: &slug,
        task_id,
        model: &ctx.settings.implement_model,
    })? {
        info!("Implementation completed");
        // Link changed files to the task (best-effort)
        match git::changed_files(&wt_path, &ctx.settings.base_branch) {
            Ok(files) if !files.is_empty() => {
                td.link(task_id, &files).ok();
            }
            Ok(_) => {}
            Err(e) => {
                warn!("Failed to get changed files for linking: {e:#}");
            }
        }
        // best-effort: orchestrator will pick up the task for review on next cycle if this fails
        if let Err(e) = td.review(task_id) {
            warn!("Failed to transition {task_id} to review: {e:#}");
        }
        info!("Task {task_id} submitted for review");
        Ok(true)
    } else {
        error!("Implementation failed (exit code nonzero)");
        let log_path_str = log_file.display().to_string();
        td.log(&format!(
            "Orchestrator: implementation failed — see {log_path_str}"
        ))
        .ok();
        td.handoff(
            task_id,
            "implementation attempted",
            &format!("implementation failed — see {log_path_str}"),
        )
        .ok();
        Ok(false)
    }
}
