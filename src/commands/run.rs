use anyhow::Result;
use tracing::info;

use crate::config::ProjectContext;
use crate::lock;
use crate::td::{NextAction, Td};
use crate::usage;

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

    // false: proposals are handled exclusively by `proposal`/`proposal-rotate` commands.
    let action = td.get_next_action(false)?;

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
        .expect("already handled Idle above")
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

        step = td.get_next_action(false)?; // proposals excluded, see above
    }

    Ok(true)
}
