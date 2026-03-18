use anyhow::Result;
use tracing::info;

use crate::config::ProjectContext;
use crate::lock;
use crate::td::{NextAction, Td};
use crate::usage;
use crate::vcs;

/// Returns Ok(true) if work was attempted, Ok(false) if nothing to do.
pub fn run(ctx: &ProjectContext) -> Result<()> {
    let slug = ctx.project_slug();
    let _lock = lock::Lock::acquire(&ctx.cfg.lock_dir, &format!("run-{slug}"))?;

    let did_work = run_inner(ctx)?;
    if !did_work {
        info!("Nothing to do — no reviewable or open tasks");
    }
    Ok(())
}

pub(crate) fn run_inner(ctx: &ProjectContext) -> Result<bool> {
    let td = Td::new(&ctx.project_root);
    let check_proposals = vcs::detect_platform(&ctx.project_root, ctx.vcs_mode).is_some();

    let action = td.get_next_action(check_proposals)?;

    // Proposal review doesn't chain into the implement/review loop.
    if let NextAction::ProposalReview(_) = &action {
        info!("Found tasks with open proposals, running proposal review");
        super::proposal_review::run_unlocked(ctx)?;
        return Ok(true);
    }

    if let NextAction::Idle = &action {
        if ctx.cfg.dry_run {
            info!("dry-run: nothing to do (no reviewable or open tasks)");
        }
        return Ok(false);
    }

    // Full flow: loop implement → review → fix → review for one task.
    // Bounded by max_reviews (task gets blocked) and usage budget.
    let task_id = action
        .task_id()
        .expect("already handled Idle and ProposalReview above")
        .to_string();
    let mut step = action;

    loop {
        let ok = match &step {
            NextAction::Implement(id) if *id == task_id => {
                super::implement::implement_task(ctx, &task_id)?
            }
            NextAction::Review(id) if *id == task_id => super::review::review_task(ctx, &task_id)?,
            other => {
                info!(
                    "Stopping full flow: next action is {} (task: {})",
                    other.label(),
                    other.task_id().unwrap_or("none")
                );
                break;
            }
        };

        if !ok {
            break;
        }

        if !usage::has_budget() {
            info!("Usage budget low, deferring remaining work to next tick");
            break;
        }

        step = td.get_next_action(check_proposals)?;
    }

    Ok(true)
}
