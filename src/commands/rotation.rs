use std::fs;
use std::path::PathBuf;

use anyhow::{Result, bail};
use tracing::{error, info};

use crate::config::{self, Config, ProjectContext};
use crate::lock;
use crate::project_config::load_project_settings;

pub fn rotate_projects(
    cfg: &Config,
    state_suffix: &str,
    lock_prefix: &str,
    action: impl Fn(&ProjectContext) -> Result<bool>,
) -> Result<()> {
    let projects = cfg.projects_list();
    if projects.is_empty() {
        bail!(
            "No projects configured. Set NOCTURNAL_PROJECTS or create {}",
            cfg.projects_file
        );
    }

    let count = projects.len();
    let state_file = format!("{}{}", cfg.rotation_state_file, state_suffix);

    let last_idx: Option<usize> = fs::read_to_string(&state_file)
        .ok()
        .and_then(|s| s.trim().parse().ok());

    if let Some(parent) = std::path::Path::new(&state_file).parent() {
        fs::create_dir_all(parent).ok();
    }

    let mut tried = 0;
    let mut idx = last_idx.map_or(0, |i| (i + 1) % count);

    while tried < count {
        let project_root = PathBuf::from(&projects[idx]);
        info!(
            "=== Rotating to project {}/{count}: {} ===",
            idx + 1,
            project_root.display()
        );

        if !project_root.join(".todos").is_dir() {
            error!(
                "td not initialized in {} — skipping",
                project_root.display()
            );
            idx = (idx + 1) % count;
            tried += 1;
            continue;
        }

        let settings = load_project_settings(&project_root);
        if !settings.auto_develop {
            info!(
                "Skipping {} — auto_develop is false",
                project_root.display()
            );
            idx = (idx + 1) % count;
            tried += 1;
            continue;
        }

        let slug = config::project_slug(&project_root);
        let lock_name = format!("{lock_prefix}-{slug}");

        let Some(_lock) = lock::Lock::try_acquire(&cfg.lock_dir, &lock_name) else {
            info!(
                "Skipping {} — locked (another process running)",
                project_root.display()
            );
            idx = (idx + 1) % count;
            tried += 1;
            continue;
        };

        let ctx = ProjectContext::new(cfg.clone(), project_root.clone());

        if cfg.dry_run {
            info!("dry-run: would process project {}", project_root.display());
            return Ok(());
        }

        fs::write(&state_file, idx.to_string()).ok();

        match action(&ctx) {
            Ok(true) => return Ok(()),
            Ok(false) => {
                info!(
                    "Nothing to do in {}, trying next project",
                    project_root.display()
                );
            }
            Err(e) => return Err(e),
        }

        idx = (idx + 1) % count;
        tried += 1;
    }

    info!("Nothing to do in any project");
    Ok(())
}
