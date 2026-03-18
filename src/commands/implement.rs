use anyhow::Result;
use tracing::{error, info};

use crate::config::ProjectContext;
use crate::{claude, git, lock, prompt, td};

pub fn run(ctx: &ProjectContext) -> Result<()> {
    let slug = ctx.project_slug();
    let _lock = lock::Lock::acquire(&ctx.cfg.lock_dir, &format!("implement-{slug}"))?;

    run_unlocked(ctx)
}

pub fn run_unlocked(ctx: &ProjectContext) -> Result<()> {
    let td = td::Td::new(&ctx.project_root);

    let task_id = td
        .get_next_task_id()?
        .ok_or_else(|| anyhow::anyhow!("No open tasks found"))?;

    info!("=== Implementing task: {task_id} ===");

    let task = td.show(&task_id)?;
    let review_count = td::get_review_count(&task);
    if review_count >= ctx.max_reviews {
        info!(
            "Task {task_id} has {review_count} review cycles (max {}), skipping",
            ctx.max_reviews
        );
        return Ok(());
    }

    if ctx.cfg.dry_run {
        info!("dry-run: would create worktree for task {task_id}");
        info!("dry-run: would invoke Claude for implement of task {task_id}");
        info!("dry-run: would submit task {task_id} for review");
        return Ok(());
    }

    let wt_path = git::ensure_worktree(&ctx.project_root, &task_id)?;
    info!("Worktree: {wt_path}");

    td.start(&task_id).ok();

    let rendered = prompt::render_base(
        prompt::Template::Implement,
        &task_id,
        &ctx.project_root,
        ctx.max_reviews,
    );

    let slug = ctx.project_slug();
    let log_file = claude::log_path(&ctx.cfg.log_dir, "implement", &task_id);

    if claude::run(
        ctx,
        &wt_path,
        &rendered,
        &log_file,
        "implement",
        &slug,
        &task_id,
    )? {
        info!("Implementation completed");
        td.review(&task_id).ok();
        info!("Task {task_id} submitted for review");
    } else {
        error!("Implementation failed (exit code nonzero)");
        td.log(&format!(
            "Orchestrator: implementation failed — see {log_file}"
        ))
        .ok();
    }

    Ok(())
}
