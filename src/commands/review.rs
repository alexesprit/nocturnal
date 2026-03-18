use anyhow::Result;
use tracing::{error, info};

use crate::config::ProjectContext;
use crate::{claude, git, lock, prompt, td};

pub fn run(ctx: &ProjectContext) -> Result<()> {
    let slug = ctx.project_slug();
    let _lock = lock::Lock::acquire(&ctx.cfg.lock_dir, &format!("review-{slug}"))?;

    run_unlocked(ctx)
}

pub fn run_unlocked(ctx: &ProjectContext) -> Result<()> {
    let td_client = td::Td::new(&ctx.project_root);

    let task_id = match td_client.get_reviewable_task_id()? {
        Some(id) => id,
        None => {
            info!("No reviewable tasks found");
            return Ok(());
        }
    };

    info!("=== Reviewing task: {task_id} ===");

    let task = td_client.show(&task_id)?;
    let review_count = td::get_review_count(&task);
    if review_count >= ctx.max_reviews {
        info!(
            "Task {task_id} reached max reviews ({review_count}/{})",
            ctx.max_reviews
        );
        td_client
            .comment(
                &task_id,
                &format!(
                    "Orchestrator: max review cycles reached ({review_count}/{}). Needs human review.",
                    ctx.max_reviews
                ),
            )
            .ok();
        td_client.block(&task_id).ok();
        return Ok(());
    }

    if ctx.cfg.dry_run {
        info!("dry-run: would invoke Claude for review of task {task_id}");
        info!("dry-run: would update task state based on review outcome");
        return Ok(());
    }

    let wt_path = git::worktree_path(&ctx.project_root, &task_id)?.ok_or_else(|| {
        anyhow::anyhow!(
            "No worktree found for {task_id} — expected branch {}",
            git::worktree_branch(&task_id)
        )
    })?;

    let review_cycle = review_count + 1;
    let rendered = prompt::render_with_review_cycle(
        prompt::Template::Review,
        &task_id,
        &ctx.project_root,
        ctx.max_reviews,
        Some(review_cycle),
    );

    let slug = ctx.project_slug();
    let log_file = claude::log_path(&ctx.cfg.log_dir, "review", &task_id);

    if !claude::run(
        ctx, &wt_path, &rendered, &log_file, "review", &slug, &task_id,
    )? {
        error!("Review failed (exit code nonzero)");
        return Ok(());
    }

    info!("Review completed");

    // Re-read task to see what the review agent decided
    let task = td_client.show(&task_id)?;

    if task.labels.iter().any(|l| l.starts_with("noc-proposal:")) {
        info!("Task {task_id} already has an open proposal — skipping re-review");
    } else if task.labels.iter().any(|l| l == "noc-proposal-ready") {
        info!("Task {task_id} passed internal review — creating proposal");
        super::proposal_review::create_proposal(ctx, &task_id)?;
    } else if task.status == "open" {
        let new_count = td::get_review_count(&task) + 1;
        let labels = td::build_labels_with_review_count(&task, new_count);
        td_client.update_labels(&task_id, &labels)?;
        info!(
            "Task rejected (review cycle {new_count}/{})",
            ctx.max_reviews
        );

        if new_count >= ctx.max_reviews {
            td_client
                .comment(
                    &task_id,
                    &format!(
                        "Orchestrator: max review cycles reached ({new_count}/{}). Needs human review.",
                        ctx.max_reviews
                    ),
                )
                .ok();
            td_client.block(&task_id).ok();
            info!("Task blocked — needs human attention");
        }
    } else {
        info!(
            "Post-review status: {} (no label action taken)",
            task.status
        );
    }

    Ok(())
}
