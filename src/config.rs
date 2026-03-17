use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Result, bail};

use crate::project_config::{self, VcsMode};

#[derive(Clone)]
pub struct Config {
    pub max_reviews: u32,
    pub max_budget: Option<u32>,
    pub model: String,
    pub lock_dir: String,
    pub log_dir: String,
    pub projects_file: String,
    pub rotation_state_file: String,
}

pub struct ProjectContext {
    pub cfg: Config,
    pub project_root: String,
    pub vcs_mode: VcsMode,
}

impl ProjectContext {
    pub fn new(cfg: Config, project_root: String) -> Self {
        let vcs_mode = project_config::load_vcs_mode(&project_root);
        Self {
            cfg,
            project_root,
            vcs_mode,
        }
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

        let home = env::var("HOME").unwrap_or_else(|_| {
            eprintln!("WARNING: HOME is not set, falling back to /tmp");
            "/tmp".to_string()
        });

        Config {
            max_reviews: env_u32("NOCTURNAL_MAX_REVIEWS", 3),
            max_budget: env::var("NOCTURNAL_MAX_BUDGET")
                .ok()
                .and_then(|v| v.parse().ok()),
            model: env::var("NOCTURNAL_MODEL").unwrap_or_else(|_| "sonnet".to_string()),
            lock_dir: env::var("NOCTURNAL_LOCK_DIR").unwrap_or_else(|_| tmpdir.clone()),
            log_dir: env::var("NOCTURNAL_LOG_DIR")
                .unwrap_or_else(|_| format!("{tmpdir}/nocturnal-logs")),
            projects_file: env::var("NOCTURNAL_PROJECTS_FILE")
                .unwrap_or_else(|_| format!("{home}/.config/nocturnal/projects")),
            rotation_state_file: env::var("NOCTURNAL_ROTATION_STATE")
                .unwrap_or_else(|_| format!("{home}/.config/nocturnal/rotation-state")),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_from_simple_path() {
        assert_eq!(project_slug("/home/user/my-project"), "my-project");
    }

    #[test]
    fn slug_strips_special_characters() {
        assert_eq!(project_slug("/path/to/my project!@#"), "myproject");
    }

    #[test]
    fn slug_preserves_underscores_and_dashes() {
        assert_eq!(project_slug("/path/my_cool-project"), "my_cool-project");
    }

    #[test]
    fn slug_from_path_without_slashes() {
        assert_eq!(project_slug("project"), "project");
    }

    #[test]
    fn slug_from_empty_string() {
        assert_eq!(project_slug(""), "");
    }

    #[test]
    fn slug_from_trailing_slash() {
        // rsplit('/').next() on "foo/" gives ""
        assert_eq!(project_slug("/path/to/project/"), "");
    }

    #[test]
    fn slug_from_root_path() {
        assert_eq!(project_slug("/"), "");
    }
}
