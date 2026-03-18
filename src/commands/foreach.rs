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
        info!("=== Project: {project_root} ===");

        if !std::path::Path::new(project_root).join(".todos").is_dir() {
            error!("td not initialized in {project_root} — skipping");
            continue;
        }

        let slug = config::project_slug(project_root);
        let lock_name = format!("run-{slug}");

        let _lock = match lock::Lock::try_acquire(&cfg.lock_dir, &lock_name) {
            Some(l) => l,
            None => {
                info!("Skipping {project_root} — locked (another process running)");
                continue;
            }
        };

        let ctx = ProjectContext::new(cfg.clone(), project_root.to_string());

        if cfg.dry_run {
            info!("dry-run: would process project {project_root}");
            continue;
        }

        match super::run::run_inner(&ctx) {
            Ok(_) => {}
            Err(e) => {
                error!("cmd_run failed for {project_root}: {e:#}");
                failed += 1;
            }
        }
    }

    if failed > 0 {
        bail!("{failed} project(s) failed");
    }
    Ok(())
}
