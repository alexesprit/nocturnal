use anyhow::Result;
use tracing::{error, info};

use crate::config::ProjectContext;
use crate::project_config::VcsMode;
use crate::{claude, git, lock, preflight, prompt, td, vcs};

pub fn run(ctx: &ProjectContext) -> Result<()> {
    let slug = ctx.project_slug();
    let _lock = lock::Lock::acquire(&ctx.cfg.lock_dir, &format!("review-{slug}"))?;

    run_unlocked(ctx)
}

fn run_unlocked(ctx: &ProjectContext) -> Result<()> {
    let td_client = td::Td::new(&ctx.project_root);

    let Some(task_id) = td_client.get_reviewable_task_id()? else {
        info!("No reviewable tasks found");
        return Ok(());
    };

    review_task(ctx, &task_id).map(|_| ())
}

/// Review a specific task. Returns Ok(true) if review completed successfully
/// (approved, rejected, or proposal created).
#[allow(clippy::too_many_lines)]
pub fn review_task(ctx: &ProjectContext, task_id: &str) -> Result<bool> {
    let td_client = td::Td::new(&ctx.project_root);

    info!("=== Reviewing task: {task_id} ===");

    let task = td_client.show(task_id)?;
    let review_count = td::get_review_count(&task);
    if review_count >= ctx.max_reviews {
        info!(
            "Task {task_id} reached max reviews ({review_count}/{})",
            ctx.max_reviews
        );
        td_client
            .comment(
                task_id,
                &format!(
                    "Orchestrator: max review cycles reached ({review_count}/{}). Needs human review.",
                    ctx.max_reviews
                ),
            )
            .ok();
        td_client.block(task_id).ok();
        return Ok(false);
    }

    if ctx.cfg.dry_run {
        info!("dry-run: would invoke Claude for review of task {task_id}");
        info!("dry-run: would update task state based on review outcome");
        return Ok(false);
    }

    preflight::run_checks(ctx)?;

    let wt_path = git::worktree_path(&ctx.project_root, task_id)?.ok_or_else(|| {
        anyhow::anyhow!(
            "No worktree found for {task_id} — expected branch {}",
            git::worktree_branch(task_id)
        )
    })?;

    let review_cycle = review_count + 1;
    let rendered = prompt::render_with_review_cycle(
        prompt::Template::Review,
        task_id,
        &ctx.project_root,
        ctx.max_reviews,
        Some(review_cycle),
        &ctx.base_branch,
    );

    let slug = ctx.project_slug();
    let log_file = claude::log_path(&ctx.cfg.log_dir, "review", task_id);

    if !ctx.review_backend.run(
        &wt_path,
        &rendered,
        &log_file,
        "review",
        &slug,
        task_id,
        &ctx.review_model,
    )? {
        error!("Review failed (exit code nonzero)");
        return Ok(false);
    }

    info!("Review completed");

    // Re-read task to see what the review agent decided
    let task = td_client.show(task_id)?;

    if task.labels.iter().any(|l| l.starts_with("noc-proposal:")) {
        info!("Task {task_id} already has an open proposal — skipping re-review");
    } else if task.status == "in_review" {
        // LLM approved the review
        match ctx.vcs_mode {
            VcsMode::Local => {
                info!("Task {task_id} approved — performing local merge");

                if !ctx.pre_merge_hooks.is_empty() {
                    info!("Running pre-merge hooks in worktree");
                    if let Err(e) = vcs::run_pre_merge_hooks(&wt_path, &ctx.pre_merge_hooks) {
                        error!("Pre-merge hook failed for {task_id}: {e:#}");
                        td_client
                            .comment(
                                task_id,
                                &format!("Orchestrator: pre-merge hook failed: {e:#}"),
                            )
                            .ok();
                        td_client.block(task_id).ok();
                        return Ok(true);
                    }
                }

                match vcs::local_merge(
                    &ctx.project_root,
                    task_id,
                    &ctx.target_branch,
                    ctx.merge_strategy,
                    &wt_path,
                ) {
                    Ok(()) => {
                        td_client.approve(task_id)?;
                        info!("Task {task_id} merged and approved");
                        vcs::run_post_merge_hooks(&ctx.project_root, &ctx.post_merge_hooks);
                    }
                    Err(e) => {
                        error!("Local merge failed for {task_id}: {e:#}");
                        let task = td_client.show(task_id)?;
                        let new_count = bump_review_count(&td_client, &task, task_id)?;

                        if new_count >= ctx.max_reviews {
                            td_client
                                .comment(
                                    task_id,
                                    &format!(
                                        "Orchestrator: local merge failed ({new_count}/{} attempts). \
                                         Needs human attention.\n\nError: {e:#}",
                                        ctx.max_reviews
                                    ),
                                )
                                .ok();
                            td_client.block(task_id)?;
                            info!(
                                "Task blocked after {new_count} merge failures — needs human attention"
                            );
                        } else {
                            td_client
                                .comment(
                                    task_id,
                                    &format!(
                                        "Orchestrator: local merge failed ({new_count}/{} attempts). \
                                         Rebase your changes onto `{}` and resolve conflicts.\n\nError: {e:#}",
                                        ctx.max_reviews, ctx.target_branch
                                    ),
                                )
                                .ok();
                            td_client.reject(
                                task_id,
                                &format!(
                                    "Merge conflict — rebase onto {} needed",
                                    ctx.target_branch
                                ),
                            )?;
                            info!(
                                "Task reopened for rebase (attempt {new_count}/{})",
                                ctx.max_reviews
                            );
                        }
                    }
                }
            }
            VcsMode::Off => {
                info!("Task {task_id} approved (VCS off) — approving task");
                td_client.approve(task_id)?;
            }
            VcsMode::Auto | VcsMode::GitHub | VcsMode::GitLab => {
                info!("Task {task_id} approved — adding noc-proposal-ready label");
                let labels =
                    td::swap_label(&task, "noc-proposal-ready", Some("noc-proposal-ready"));
                td_client.update_labels(task_id, &labels)?;
                info!("Task {task_id} passed internal review — creating proposal");
                super::proposal_review::create_proposal(ctx, task_id)?;
            }
        }
    } else if task.status == "open" {
        let new_count = bump_review_count(&td_client, &task, task_id)?;
        info!(
            "Task rejected (review cycle {new_count}/{})",
            ctx.max_reviews
        );

        if new_count >= ctx.max_reviews {
            td_client
                .comment(
                    task_id,
                    &format!(
                        "Orchestrator: max review cycles reached ({new_count}/{}). Needs human review.",
                        ctx.max_reviews
                    ),
                )
                .ok();
            td_client.block(task_id).ok();
            info!("Task blocked — needs human attention");
        }
    } else {
        info!(
            "Post-review status: {} (no label action taken)",
            task.status
        );
    }

    Ok(true)
}

/// Increment the `noc-reviews` label counter and persist it. Returns the new count.
fn bump_review_count(td_client: &td::Td, task: &td::Task, task_id: &str) -> Result<u32> {
    let new_count = td::get_review_count(task) + 1;
    let labels = td::build_labels_with_review_count(task, new_count);
    td_client.update_labels(task_id, &labels)?;
    Ok(new_count)
}
