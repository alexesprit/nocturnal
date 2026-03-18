use std::io;
use std::path::Path;

use serde::Deserialize;

pub const DEFAULT_MAX_REVIEWS: u32 = 3;
pub const DEFAULT_MAX_BUDGET: Option<u32> = None;
pub const DEFAULT_MODEL: &str = "sonnet";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VcsMode {
    #[default]
    Off,
    Auto,
    GitHub,
    GitLab,
}

#[derive(Debug, Deserialize)]
struct ProjectConfig {
    vcs: Option<VcsMode>,
    max_reviews: Option<u32>,
    max_budget: Option<u32>,
    model: Option<String>,
}

pub struct ProjectSettings {
    pub vcs: VcsMode,
    pub max_reviews: u32,
    pub max_budget: Option<u32>,
    pub model: String,
}

pub fn load_project_settings(project_root: &str) -> ProjectSettings {
    let path = Path::new(project_root).join(".nocturnal.toml");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return ProjectSettings::default(),
        Err(e) => {
            tracing::warn!("Failed to read {}: {e}", path.display());
            return ProjectSettings::default();
        }
    };
    match toml::from_str::<ProjectConfig>(&content) {
        Ok(f) => ProjectSettings {
            vcs: f.vcs.unwrap_or_default(),
            max_reviews: f.max_reviews.unwrap_or(DEFAULT_MAX_REVIEWS),
            max_budget: f.max_budget.or(DEFAULT_MAX_BUDGET),
            model: f.model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        },
        Err(e) => {
            tracing::warn!("Failed to parse {}: {e}", path.display());
            ProjectSettings::default()
        }
    }
}

impl Default for ProjectSettings {
    fn default() -> Self {
        ProjectSettings {
            vcs: VcsMode::default(),
            max_reviews: DEFAULT_MAX_REVIEWS,
            max_budget: DEFAULT_MAX_BUDGET,
            model: DEFAULT_MODEL.to_string(),
        }
    }
}

/// Convenience wrapper kept for callers that only need vcs mode.
pub fn load_vcs_mode(project_root: &str) -> VcsMode {
    load_project_settings(project_root).vcs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_vcs_off() {
        let f: ProjectConfig = toml::from_str("vcs = \"off\"").unwrap();
        assert_eq!(f.vcs.unwrap(), VcsMode::Off);
    }

    #[test]
    fn parse_vcs_auto() {
        let f: ProjectConfig = toml::from_str("vcs = \"auto\"").unwrap();
        assert_eq!(f.vcs.unwrap(), VcsMode::Auto);
    }

    #[test]
    fn parse_vcs_github() {
        let f: ProjectConfig = toml::from_str("vcs = \"github\"").unwrap();
        assert_eq!(f.vcs.unwrap(), VcsMode::GitHub);
    }

    #[test]
    fn parse_vcs_gitlab() {
        let f: ProjectConfig = toml::from_str("vcs = \"gitlab\"").unwrap();
        assert_eq!(f.vcs.unwrap(), VcsMode::GitLab);
    }

    #[test]
    fn parse_missing_vcs() {
        let f: ProjectConfig = toml::from_str("").unwrap();
        assert_eq!(f.vcs.unwrap_or_default(), VcsMode::Off);
    }

    #[test]
    fn parse_unrecognized_value_is_error() {
        assert!(toml::from_str::<ProjectConfig>("vcs = \"bitbucket\"").is_err());
    }

    #[test]
    fn load_from_nonexistent_dir() {
        assert_eq!(load_vcs_mode("/nonexistent/path"), VcsMode::Off);
    }

    #[test]
    fn parse_full_config() {
        let f: ProjectConfig = toml::from_str(
            "vcs = \"gitlab\"\nmax_reviews = 5\nmax_budget = 10\nmodel = \"opus\"",
        )
        .unwrap();
        assert_eq!(f.vcs.unwrap(), VcsMode::GitLab);
        assert_eq!(f.max_reviews.unwrap(), 5);
        assert_eq!(f.max_budget.unwrap(), 10);
        assert_eq!(f.model.unwrap(), "opus");
    }

    #[test]
    fn defaults_when_fields_missing() {
        let settings = load_project_settings("/nonexistent/path");
        assert_eq!(settings.max_reviews, DEFAULT_MAX_REVIEWS);
        assert_eq!(settings.max_budget, DEFAULT_MAX_BUDGET);
        assert_eq!(settings.model, DEFAULT_MODEL);
    }

    #[test]
    fn parse_partial_config() {
        let f: ProjectConfig = toml::from_str("max_reviews = 7").unwrap();
        assert_eq!(f.max_reviews.unwrap(), 7);
        assert!(f.max_budget.is_none());
        assert!(f.model.is_none());
        assert!(f.vcs.is_none());
    }
}
