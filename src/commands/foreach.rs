use std::path::PathBuf;

use anyhow::{Result, bail};
use tracing::{error, info};

use crate::config::{self, Config, ProjectContext};
use crate::lock;

pub fn run(cfg: &Config) -> Result<()> {
    let projects = cfg.projects_list();
    if projects.is_empty() {
        bail!(
            "No projects configured. Set NOCTURNAL_PROJECTS (colon-separated) or create {}",
            cfg.projects_file
        );
    }

    let mut failed = 0;

    for project_root in &projects {
        let project_root = PathBuf::from(project_root);
        info!("=== Project: {} ===", project_root.display());

        if !project_root.join(".todos").is_dir() {
            error!(
                "td not initialized in {} — skipping",
                project_root.display()
            );
            continue;
        }

        let slug = config::project_slug(&project_root);
        let lock_name = format!("run-{slug}");

        let _lock = match lock::Lock::try_acquire(&cfg.lock_dir, &lock_name) {
            Some(l) => l,
            None => {
                info!(
                    "Skipping {} — locked (another process running)",
                    project_root.display()
                );
                continue;
            }
        };

        let ctx = ProjectContext::new(cfg.clone(), project_root.clone());

        if cfg.dry_run {
            info!("dry-run: would process project {}", project_root.display());
            continue;
        }

        match super::run::run_inner(&ctx) {
            Ok(_) => {}
            Err(e) => {
                error!("cmd_run failed for {}: {e:#}", project_root.display());
                failed += 1;
            }
        }
    }

    if failed > 0 {
        bail!("{failed} project(s) failed");
    }
    Ok(())
}
