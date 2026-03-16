use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Result, bail};

#[derive(Clone)]
pub struct Config {
    pub max_reviews: u32,
    pub max_budget: u32,
    pub model: String,
    pub lock_dir: String,
    pub log_dir: String,
    pub projects_file: String,
    pub rotation_state_file: String,
    pub vcs_platform_override: Option<VcsPlatformOverride>,
}

#[derive(Clone)]
pub enum VcsPlatformOverride {
    Disabled,
    Forced(String),
}

pub struct ProjectContext {
    pub cfg: Config,
    pub project_root: String,
}

impl ProjectContext {
    pub fn new(cfg: Config, project_root: String) -> Self {
        Self { cfg, project_root }
    }

    pub fn project_slug(&self) -> String {
        project_slug(&self.project_root)
    }
}

pub fn project_slug(project_root: &str) -> String {
    project_root
        .rsplit('/')
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect()
}

impl Config {
    pub fn from_env() -> Self {
        let tmpdir = env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());

        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());

        let vcs_platform_override =
            env::var("NOCTURNAL_VCS_PLATFORM")
                .ok()
                .map(|v| match v.as_str() {
                    "none" | "off" | "disabled" => VcsPlatformOverride::Disabled,
                    other => VcsPlatformOverride::Forced(other.to_string()),
                });

        Config {
            max_reviews: env_u32("NOCTURNAL_MAX_REVIEWS", 3),
            max_budget: env_u32("NOCTURNAL_MAX_BUDGET", 5),
            model: env::var("NOCTURNAL_MODEL").unwrap_or_else(|_| "sonnet".to_string()),
            lock_dir: env::var("NOCTURNAL_LOCK_DIR").unwrap_or_else(|_| tmpdir.clone()),
            log_dir: env::var("NOCTURNAL_LOG_DIR")
                .unwrap_or_else(|_| format!("{tmpdir}/nocturnal-logs")),
            projects_file: env::var("NOCTURNAL_PROJECTS_FILE")
                .unwrap_or_else(|_| format!("{home}/.config/nocturnal/projects")),
            rotation_state_file: env::var("NOCTURNAL_ROTATION_STATE")
                .unwrap_or_else(|_| format!("{home}/.config/nocturnal/rotation-state")),
            vcs_platform_override,
        }
    }

    pub fn projects_list(&self) -> Vec<String> {
        if let Ok(val) = env::var("NOCTURNAL_PROJECTS") {
            return val.split(':').map(|s| s.to_string()).collect();
        }
        if let Ok(content) = fs::read_to_string(&self.projects_file) {
            return content
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|l| l.to_string())
                .collect();
        }
        Vec::new()
    }
}

fn env_u32(key: &str, default: u32) -> u32 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

pub fn check_td_init(project_root: &str) -> Result<()> {
    if !Path::new(project_root).join(".todos").is_dir() {
        bail!("td not initialized in {project_root} (run 'td init')");
    }
    Ok(())
}
