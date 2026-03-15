use std::fs;

use anyhow::{Result, bail};
use tracing::{error, info};

use crate::config::{self, Config, ProjectContext};
use crate::lock;

pub fn run(cfg: &Config) -> Result<()> {
    let projects = cfg.projects_list();
    if projects.is_empty() {
        bail!(
            "No projects configured. Set NOCTURNAL_PROJECTS or create {}",
            cfg.projects_file
        );
    }

    let count = projects.len();

    let last_idx: i64 = fs::read_to_string(&cfg.rotation_state_file)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(-1);

    if let Some(parent) = std::path::Path::new(&cfg.rotation_state_file).parent() {
        fs::create_dir_all(parent).ok();
    }

    let mut tried = 0;
    let mut idx = ((last_idx + 1) % count as i64) as usize;

    while tried < count {
        let project_root = &projects[idx];
        info!(
            "=== Rotating to project {}/{count}: {project_root} ===",
            idx + 1
        );

        fs::write(&cfg.rotation_state_file, idx.to_string()).ok();

        if !std::path::Path::new(project_root).join(".todos").is_dir() {
            error!("td not initialized in {project_root} — skipping");
            idx = (idx + 1) % count;
            tried += 1;
            continue;
        }

        let slug = config::project_slug(project_root);
        let lock_name = format!("run-{slug}");

        let _lock = match lock::Lock::try_acquire(&cfg.lock_dir, &lock_name) {
            Some(l) => l,
            None => {
                info!("Skipping {project_root} — locked (another process running)");
                idx = (idx + 1) % count;
                tried += 1;
                continue;
            }
        };

        let ctx = ProjectContext::new(
            Config::from_env(), // fresh config per project
            project_root.clone(),
        );

        match super::run::run_inner(&ctx) {
            Ok(true) => return Ok(()),
            Ok(false) => {
                info!("Nothing to do in {project_root}, trying next project");
            }
            Err(e) => return Err(e),
        }

        idx = (idx + 1) % count;
        tried += 1;
    }

    info!("Nothing to do in any project");
    Ok(())
}
