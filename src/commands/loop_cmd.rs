use anyhow::Result;
use tracing::info;

use crate::config::{Config, ProjectContext};

pub fn run(cfg: &Config, max_iterations: Option<usize>) -> Result<()> {
    let mut i = 0;

    loop {
        if let Some(max) = max_iterations {
            if i >= max {
                info!("Loop reached max iterations ({max}), stopping");
                break;
            }
            info!("Loop iteration {}/{max}", i + 1);
        } else {
            info!("Loop iteration {}", i + 1);
        }

        let processed = super::rotation::rotate_projects(cfg, "", "run", |ctx| {
            super::run::run_inner(ctx, None)
        })?;

        if !processed {
            info!("Loop stopping — nothing to do in any project");
            break;
        }

        i += 1;
    }

    Ok(())
}

pub fn run_single(ctx: &ProjectContext, max_iterations: Option<usize>) -> Result<()> {
    let slug = ctx.project_slug();
    let _lock = crate::lock::Lock::acquire(&ctx.cfg.lock_dir, &format!("run-{slug}"))?;

    let mut i = 0;

    loop {
        if let Some(max) = max_iterations {
            if i >= max {
                info!("Loop reached max iterations ({max}), stopping");
                break;
            }
            info!("Loop iteration {}/{max}", i + 1);
        } else {
            info!("Loop iteration {}", i + 1);
        }

        let did_work = super::run::run_inner(ctx, None)?;

        if !did_work {
            info!("Loop stopping — nothing to do");
            break;
        }

        i += 1;
    }

    Ok(())
}
