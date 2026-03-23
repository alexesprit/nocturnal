use anyhow::Result;

use crate::config::Config;

pub fn run(cfg: &Config) -> Result<()> {
    super::rotation::rotate_projects(cfg, "", "run", |ctx| super::run::run_inner(ctx))
}
