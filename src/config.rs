use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, bail};

use crate::backend::{AiBackend, ClaudeBackend, CodexBackend};
use crate::project_config::{self, ProjectSettings, Provider};

#[derive(Clone)]
pub struct Config {
    pub lock_dir: PathBuf,
    pub log_dir: PathBuf,
    pub projects_file: String,
    pub rotation_state_file: String,
    pub dry_run: bool,
}

pub struct ProjectContext {
    pub cfg: Config,
    pub project_root: PathBuf,
    pub settings: ProjectSettings,
    pub implement_backend: Arc<dyn AiBackend>,
    pub review_backend: Arc<dyn AiBackend>,
}

impl ProjectContext {
    pub fn new(cfg: Config, project_root: PathBuf) -> Self {
        let settings = project_config::load_project_settings(&project_root);
        let make_backend = |provider: Provider| -> Arc<dyn AiBackend> {
            match provider {
                Provider::Claude => Arc::new(ClaudeBackend {
                    max_budget: settings.max_budget,
                }),
                Provider::Codex => Arc::new(CodexBackend {
                    max_budget: settings.max_budget,
                    reasoning_effort: settings.codex_reasoning_effort.clone(),
                }),
            }
        };
        let implement_backend = make_backend(settings.implement_provider);
        let review_backend = if settings.review_provider == settings.implement_provider {
            Arc::clone(&implement_backend)
        } else {
            make_backend(settings.review_provider)
        };
        Self {
            cfg,
            project_root,
            settings,
            implement_backend,
            review_backend,
        }
    }

    pub fn project_slug(&self) -> String {
        project_slug(&self.project_root)
    }
}

pub fn project_slug(project_root: &Path) -> String {
    project_root
        .file_name()
        .and_then(|n| n.to_str())
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
            lock_dir: PathBuf::from(
                env::var("NOCTURNAL_LOCK_DIR").unwrap_or_else(|_| tmpdir.clone()),
            ),
            log_dir: PathBuf::from(
                env::var("NOCTURNAL_LOG_DIR")
                    .unwrap_or_else(|_| format!("{tmpdir}/nocturnal-logs")),
            ),
            projects_file: env::var("NOCTURNAL_PROJECTS_FILE")
                .unwrap_or_else(|_| format!("{home}/.config/nocturnal/projects")),
            rotation_state_file: env::var("NOCTURNAL_ROTATION_STATE")
                .unwrap_or_else(|_| format!("{home}/.config/nocturnal/rotation-state")),
            dry_run: false,
        }
    }

    pub fn projects_list(&self) -> Vec<String> {
        if let Ok(val) = env::var("NOCTURNAL_PROJECTS") {
            return val
                .split(':')
                .map(std::string::ToString::to_string)
                .collect();
        }
        if let Ok(content) = fs::read_to_string(&self.projects_file) {
            return content
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(std::string::ToString::to_string)
                .collect();
        }
        Vec::new()
    }
}

pub fn check_td_init(project_root: &Path) -> Result<()> {
    if !project_root.join(".todos").is_dir() {
        bail!(
            "td not initialized in {} (run 'td init')",
            project_root.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_from_simple_path() {
        assert_eq!(
            project_slug(Path::new("/home/user/my-project")),
            "my-project"
        );
    }

    #[test]
    fn slug_strips_special_characters() {
        assert_eq!(
            project_slug(Path::new("/path/to/my project!@#")),
            "myproject"
        );
    }

    #[test]
    fn slug_preserves_underscores_and_dashes() {
        assert_eq!(
            project_slug(Path::new("/path/my_cool-project")),
            "my_cool-project"
        );
    }

    #[test]
    fn slug_from_path_without_slashes() {
        assert_eq!(project_slug(Path::new("project")), "project");
    }

    #[test]
    fn slug_from_empty_string() {
        assert_eq!(project_slug(Path::new("")), "");
    }

    #[test]
    fn slug_from_trailing_slash() {
        // Path::file_name() strips trailing slashes, so this now returns the correct slug
        assert_eq!(project_slug(Path::new("/path/to/project/")), "project");
    }

    #[test]
    fn slug_from_root_path() {
        assert_eq!(project_slug(Path::new("/")), "");
    }
}
