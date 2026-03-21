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

#[derive(Debug, Default, Deserialize)]
struct VcsConfig {
    mode: Option<VcsMode>,
    auto_merge: Option<bool>,
    delete_branch_on_merge: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ProjectConfig {
    vcs: Option<VcsConfig>,
    max_reviews: Option<u32>,
    max_budget: Option<u32>,
    model: Option<String>,
}

pub struct ProjectSettings {
    pub vcs_mode: VcsMode,
    pub auto_merge: bool,
    pub delete_branch_on_merge: bool,
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
        Ok(f) => {
            let vcs = f.vcs.unwrap_or_default();
            ProjectSettings {
                vcs_mode: vcs.mode.unwrap_or_default(),
                auto_merge: vcs.auto_merge.unwrap_or(true),
                delete_branch_on_merge: vcs.delete_branch_on_merge.unwrap_or(false),
                max_reviews: f.max_reviews.unwrap_or(DEFAULT_MAX_REVIEWS),
                max_budget: f.max_budget.or(DEFAULT_MAX_BUDGET),
                model: f.model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            }
        }
        Err(e) => {
            tracing::warn!("Failed to parse {}: {e}", path.display());
            ProjectSettings::default()
        }
    }
}

impl Default for ProjectSettings {
    fn default() -> Self {
        ProjectSettings {
            vcs_mode: VcsMode::default(),
            auto_merge: true,
            delete_branch_on_merge: false,
            max_reviews: DEFAULT_MAX_REVIEWS,
            max_budget: DEFAULT_MAX_BUDGET,
            model: DEFAULT_MODEL.to_string(),
        }
    }
}

/// Convenience wrapper kept for callers that only need vcs mode.
pub fn load_vcs_mode(project_root: &str) -> VcsMode {
    load_project_settings(project_root).vcs_mode
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_vcs_modes() {
        for (input, expected) in [
            ("off", VcsMode::Off),
            ("auto", VcsMode::Auto),
            ("github", VcsMode::GitHub),
            ("gitlab", VcsMode::GitLab),
        ] {
            let f: ProjectConfig = toml::from_str(&format!("[vcs]\nmode = \"{input}\"")).unwrap();
            assert_eq!(f.vcs.unwrap().mode.unwrap(), expected);
        }
    }

    #[test]
    fn parse_missing_vcs() {
        let f: ProjectConfig = toml::from_str("").unwrap();
        assert_eq!(
            f.vcs.unwrap_or_default().mode.unwrap_or_default(),
            VcsMode::Off
        );
    }

    #[test]
    fn parse_unrecognized_vcs_mode_is_error() {
        assert!(toml::from_str::<ProjectConfig>("[vcs]\nmode = \"bitbucket\"").is_err());
    }

    #[test]
    fn load_from_nonexistent_dir() {
        assert_eq!(load_vcs_mode("/nonexistent/path"), VcsMode::Off);
    }

    #[test]
    fn parse_full_config() {
        let f: ProjectConfig = toml::from_str(
            "max_reviews = 5\nmax_budget = 10\nmodel = \"opus\"\n\n[vcs]\nmode = \"gitlab\"\nauto_merge = false",
        )
        .unwrap();
        let vcs = f.vcs.unwrap();
        assert_eq!(vcs.mode.unwrap(), VcsMode::GitLab);
        assert!(!vcs.auto_merge.unwrap());
        assert_eq!(f.max_reviews.unwrap(), 5);
        assert_eq!(f.max_budget.unwrap(), 10);
        assert_eq!(f.model.unwrap(), "opus");
    }

    #[test]
    fn auto_merge_defaults_to_true() {
        let settings = load_project_settings("/nonexistent/path");
        assert!(settings.auto_merge);
    }

    #[test]
    fn vcs_auto_merge_false() {
        let f: ProjectConfig =
            toml::from_str("[vcs]\nmode = \"github\"\nauto_merge = false").unwrap();
        let vcs = f.vcs.unwrap();
        assert!(!vcs.auto_merge.unwrap());
    }

    #[test]
    fn empty_vcs_section_uses_defaults() {
        let f: ProjectConfig = toml::from_str("[vcs]").unwrap();
        let vcs = f.vcs.unwrap();
        assert_eq!(vcs.mode.unwrap_or_default(), VcsMode::Off);
        assert!(vcs.auto_merge.is_none());
    }

    #[test]
    fn vcs_auto_merge_only_defaults_mode_to_off() {
        let f: ProjectConfig = toml::from_str("[vcs]\nauto_merge = false").unwrap();
        let vcs = f.vcs.unwrap();
        assert_eq!(vcs.mode.unwrap_or_default(), VcsMode::Off);
        assert!(!vcs.auto_merge.unwrap());
    }

    #[test]
    fn delete_branch_on_merge_defaults_to_false() {
        let settings = load_project_settings("/nonexistent/path");
        assert!(!settings.delete_branch_on_merge);
    }

    #[test]
    fn parse_delete_branch_on_merge_true() {
        let f: ProjectConfig =
            toml::from_str("[vcs]\nmode = \"github\"\ndelete_branch_on_merge = true").unwrap();
        let vcs = f.vcs.unwrap();
        assert!(vcs.delete_branch_on_merge.unwrap());
    }

    #[test]
    fn parse_delete_branch_on_merge_false() {
        let f: ProjectConfig =
            toml::from_str("[vcs]\nmode = \"github\"\ndelete_branch_on_merge = false").unwrap();
        let vcs = f.vcs.unwrap();
        assert!(!vcs.delete_branch_on_merge.unwrap());
    }

    #[test]
    fn empty_vcs_section_delete_branch_on_merge_is_none() {
        let f: ProjectConfig = toml::from_str("[vcs]").unwrap();
        let vcs = f.vcs.unwrap();
        assert!(vcs.delete_branch_on_merge.is_none());
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
