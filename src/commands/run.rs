use anyhow::Result;
use tracing::info;

use crate::config::ProjectContext;
use crate::lock;
use crate::td::{NextAction, Td};
use crate::vcs;

/// Returns Ok(true) if work was done, Ok(false) if nothing to do.
pub fn run(ctx: &ProjectContext) -> Result<()> {
    let slug = ctx.project_slug();
    let _lock = lock::Lock::acquire(&ctx.cfg.lock_dir, &format!("run-{slug}"))?;

    let did_work = run_inner(ctx)?;
    if !did_work {
        info!("Nothing to do — no reviewable or open tasks");
    }
    Ok(())
}

pub fn run_inner(ctx: &ProjectContext) -> Result<bool> {
    let td = Td::new(&ctx.project_root);
    let check_proposals = vcs::detect_platform(&ctx.project_root, ctx.vcs_mode).is_some();
    let action = td.get_next_action(check_proposals)?;

    match &action {
        NextAction::ProposalReview(_) => {
            info!("Found tasks with open proposals, running proposal review");
            super::proposal_review::run_unlocked(ctx)?;
        }
        NextAction::Review(task_id) => {
            info!("Found reviewable task ({task_id}), running review");
            super::review::run_unlocked(ctx)?;
        }
        NextAction::Implement(task_id) => {
            info!("No reviewable tasks, implementing next ({task_id})");
            super::implement::run_unlocked(ctx)?;
        }
        NextAction::Idle => return Ok(false),
    }

    Ok(true)
}
