use anyhow::Result;
use tracing::info;

use crate::config::ProjectContext;
use crate::lock;
use crate::td::Td;
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

    // Check for tasks with open proposals first
    let platform = vcs::detect_platform(&ctx.project_root, &ctx.cfg.vcs_platform_override);
    if platform.is_some()
        && let Some(task_id) = td.get_proposal_task_id()?
    {
        info!("Found task with open proposal ({task_id}), running proposal review");
        super::proposal_review::run_unlocked(ctx)?;
        return Ok(true);
    }

    // Priority: review first, then implement
    if let Some(task_id) = td.get_reviewable_task_id()? {
        info!("Found reviewable task ({task_id}), running review");
        super::review::run_unlocked(ctx)?;
        return Ok(true);
    }

    if let Some(task_id) = td.get_next_task_id()? {
        info!("No reviewable tasks, implementing next ({task_id})");
        super::implement::run_unlocked(ctx)?;
        return Ok(true);
    }

    Ok(false)
}
