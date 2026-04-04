use anyhow::Result;

use crate::config::Config;

pub fn run(cfg: &Config) -> Result<()> {
    super::rotation::rotate_projects(cfg, "-proposal", "proposal", |ctx| {
        super::proposal_review::run_unlocked(ctx)
    })?;
    Ok(())
}
