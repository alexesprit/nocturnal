use std::io;
use std::path::Path;

use serde::Deserialize;

pub const DEFAULT_MAX_REVIEWS: u32 = 3;
pub const DEFAULT_MAX_BUDGET: Option<u32> = None;
pub const DEFAULT_MODEL: &str = "sonnet";
pub const DEFAULT_TARGET_BRANCH: &str = "main";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VcsMode {
    #[default]
    Off,
    Auto,
    GitHub,
    GitLab,
    Local,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub enum MergeStrategy {
    #[default]
    #[serde(rename = "ff")]
    Ff,
    #[serde(rename = "no-ff")]
    NoFf,
    #[serde(rename = "rebase")]
    Rebase,
}

#[derive(Debug, Default, Deserialize)]
struct VcsConfig {
    mode: Option<VcsMode>,
    auto_merge: Option<bool>,
    delete_branch_on_merge: Option<bool>,
    target_branch: Option<String>,
    merge_strategy: Option<MergeStrategy>,
}

#[derive(Debug, Default, Deserialize)]
struct HooksConfig {
    post_merge: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct ClaudeConfig {
    model: Option<String>,
    implement_model: Option<String>,
    review_model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectConfig {
    vcs: Option<VcsConfig>,
    hooks: Option<HooksConfig>,
    max_reviews: Option<u32>,
    max_budget: Option<u32>,
    claude: Option<ClaudeConfig>,
}

pub struct ProjectSettings {
    pub vcs_mode: VcsMode,
    pub auto_merge: bool,
    pub delete_branch_on_merge: bool,
    pub target_branch: String,
    pub merge_strategy: MergeStrategy,
    pub max_reviews: u32,
    pub max_budget: Option<u32>,
    pub implement_model: String,
    pub review_model: String,
    pub post_merge_hooks: Vec<String>,
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
            let claude = f.claude.unwrap_or_default();
            let default_model = claude.model.as_deref().unwrap_or(DEFAULT_MODEL);
            let hooks = f.hooks.unwrap_or_default();
            ProjectSettings {
                vcs_mode: vcs.mode.unwrap_or_default(),
                auto_merge: vcs.auto_merge.unwrap_or(true),
                delete_branch_on_merge: vcs.delete_branch_on_merge.unwrap_or(false),
                target_branch: vcs
                    .target_branch
                    .filter(|b| {
                        if validate_branch_name(b) {
                            true
                        } else {
                            tracing::warn!("Invalid target_branch {b:?}, using default");
                            false
                        }
                    })
                    .unwrap_or_else(|| DEFAULT_TARGET_BRANCH.to_string()),
                merge_strategy: vcs.merge_strategy.unwrap_or_default(),
                max_reviews: f.max_reviews.unwrap_or(DEFAULT_MAX_REVIEWS),
                max_budget: f.max_budget.or(DEFAULT_MAX_BUDGET),
                implement_model: claude
                    .implement_model
                    .unwrap_or_else(|| default_model.to_string()),
                review_model: claude
                    .review_model
                    .unwrap_or_else(|| default_model.to_string()),
                post_merge_hooks: hooks.post_merge.unwrap_or_default(),
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
            target_branch: DEFAULT_TARGET_BRANCH.to_string(),
            merge_strategy: MergeStrategy::default(),
            max_reviews: DEFAULT_MAX_REVIEWS,
            max_budget: DEFAULT_MAX_BUDGET,
            implement_model: DEFAULT_MODEL.to_string(),
            review_model: DEFAULT_MODEL.to_string(),
            post_merge_hooks: Vec::new(),
        }
    }
}

/// Validate that a branch name doesn't contain problematic characters.
/// Rejects characters forbidden by git ref format rules.
fn validate_branch_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && !name.contains("@{")
        && !name.starts_with('.')
        && !name.ends_with('.')
        && !name.ends_with(".lock")
        && !name.contains(|c: char| {
            c.is_whitespace()
                || c.is_control()
                || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        })
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
            ("local", VcsMode::Local),
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
            "max_reviews = 5\nmax_budget = 10\n\n[vcs]\nmode = \"gitlab\"\nauto_merge = false\n\n[claude]\nmodel = \"opus\"",
        )
        .unwrap();
        let vcs = f.vcs.unwrap();
        assert_eq!(vcs.mode.unwrap(), VcsMode::GitLab);
        assert!(!vcs.auto_merge.unwrap());
        assert_eq!(f.max_reviews.unwrap(), 5);
        assert_eq!(f.max_budget.unwrap(), 10);
        assert_eq!(f.claude.unwrap().model.unwrap(), "opus");
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
        assert_eq!(settings.implement_model, DEFAULT_MODEL);
        assert_eq!(settings.review_model, DEFAULT_MODEL);
        assert_eq!(settings.target_branch, DEFAULT_TARGET_BRANCH);
        assert_eq!(settings.merge_strategy, MergeStrategy::Ff);
        assert!(settings.post_merge_hooks.is_empty());
    }

    #[test]
    fn parse_partial_config() {
        let f: ProjectConfig = toml::from_str("max_reviews = 7").unwrap();
        assert_eq!(f.max_reviews.unwrap(), 7);
        assert!(f.max_budget.is_none());
        assert!(f.claude.is_none());
        assert!(f.vcs.is_none());
    }

    #[test]
    fn claude_section_model_fallback() {
        let toml = "[claude]\nmodel = \"opus\"";
        let settings_toml = format!("{toml}");
        // Use a temp dir approach: write to a temp file and load
        // Instead, test the struct directly
        let f: ProjectConfig = toml::from_str(toml).unwrap();
        let claude = f.claude.unwrap();
        assert_eq!(claude.model.as_deref(), Some("opus"));
        assert!(claude.implement_model.is_none());
        assert!(claude.review_model.is_none());
        // Resolution: implement_model falls back to model
        let default_model = claude.model.as_deref().unwrap_or(DEFAULT_MODEL);
        let implement_model = claude
            .implement_model
            .unwrap_or_else(|| default_model.to_string());
        assert_eq!(implement_model, "opus");
        drop(settings_toml);
    }

    #[test]
    fn claude_section_per_operation_override() {
        let f: ProjectConfig = toml::from_str(
            "[claude]\nmodel = \"sonnet\"\nimplement_model = \"opus\"\nreview_model = \"haiku\"",
        )
        .unwrap();
        let claude = f.claude.unwrap();
        assert_eq!(claude.model.as_deref(), Some("sonnet"));
        assert_eq!(claude.implement_model.as_deref(), Some("opus"));
        assert_eq!(claude.review_model.as_deref(), Some("haiku"));
    }

    #[test]
    fn empty_claude_section_uses_default_model() {
        let f: ProjectConfig = toml::from_str("[claude]").unwrap();
        let claude = f.claude.unwrap();
        assert!(claude.model.is_none());
        assert!(claude.implement_model.is_none());
        assert!(claude.review_model.is_none());
        let default_model = claude.model.as_deref().unwrap_or(DEFAULT_MODEL);
        assert_eq!(default_model, DEFAULT_MODEL);
    }

    #[test]
    fn no_claude_section_uses_default_model() {
        let settings = load_project_settings("/nonexistent/path");
        assert_eq!(settings.implement_model, DEFAULT_MODEL);
        assert_eq!(settings.review_model, DEFAULT_MODEL);
    }

    // --- MergeStrategy ---

    #[test]
    fn parse_merge_strategies() {
        for (input, expected) in [
            ("ff", MergeStrategy::Ff),
            ("no-ff", MergeStrategy::NoFf),
            ("rebase", MergeStrategy::Rebase),
        ] {
            let f: ProjectConfig =
                toml::from_str(&format!("[vcs]\nmerge_strategy = \"{input}\"")).unwrap();
            assert_eq!(f.vcs.unwrap().merge_strategy.unwrap(), expected);
        }
    }

    #[test]
    fn parse_unrecognized_merge_strategy_is_error() {
        assert!(toml::from_str::<ProjectConfig>("[vcs]\nmerge_strategy = \"squash\"").is_err());
    }

    #[test]
    fn merge_strategy_defaults_to_ff() {
        let settings = load_project_settings("/nonexistent/path");
        assert_eq!(settings.merge_strategy, MergeStrategy::Ff);
    }

    // --- target_branch ---

    #[test]
    fn parse_target_branch() {
        let f: ProjectConfig =
            toml::from_str("[vcs]\nmode = \"local\"\ntarget_branch = \"develop\"").unwrap();
        assert_eq!(f.vcs.unwrap().target_branch.unwrap(), "develop");
    }

    #[test]
    fn target_branch_defaults_to_main() {
        let settings = load_project_settings("/nonexistent/path");
        assert_eq!(settings.target_branch, "main");
    }

    // --- Local VCS mode ---

    #[test]
    fn parse_local_vcs_mode() {
        let f: ProjectConfig = toml::from_str("[vcs]\nmode = \"local\"").unwrap();
        assert_eq!(f.vcs.unwrap().mode.unwrap(), VcsMode::Local);
    }

    #[test]
    fn parse_local_mode_with_merge_strategy() {
        let f: ProjectConfig = toml::from_str(
            "[vcs]\nmode = \"local\"\ntarget_branch = \"develop\"\nmerge_strategy = \"no-ff\"",
        )
        .unwrap();
        let vcs = f.vcs.unwrap();
        assert_eq!(vcs.mode.unwrap(), VcsMode::Local);
        assert_eq!(vcs.target_branch.unwrap(), "develop");
        assert_eq!(vcs.merge_strategy.unwrap(), MergeStrategy::NoFf);
    }

    // --- HooksConfig ---

    #[test]
    fn parse_hooks_config() {
        let f: ProjectConfig =
            toml::from_str("[hooks]\npost_merge = [\"git push\", \"just install\"]").unwrap();
        let hooks = f.hooks.unwrap();
        assert_eq!(hooks.post_merge.unwrap(), vec!["git push", "just install"]);
    }

    #[test]
    fn parse_empty_hooks() {
        let f: ProjectConfig = toml::from_str("[hooks]").unwrap();
        let hooks = f.hooks.unwrap();
        assert!(hooks.post_merge.is_none());
    }

    #[test]
    fn parse_hooks_empty_list() {
        let f: ProjectConfig = toml::from_str("[hooks]\npost_merge = []").unwrap();
        let hooks = f.hooks.unwrap();
        assert!(hooks.post_merge.unwrap().is_empty());
    }

    #[test]
    fn post_merge_hooks_default_to_empty() {
        let settings = load_project_settings("/nonexistent/path");
        assert!(settings.post_merge_hooks.is_empty());
    }

    #[test]
    fn validate_branch_name_rejects_invalid() {
        assert!(!validate_branch_name(""));
        assert!(!validate_branch_name("main..dev"));
        assert!(!validate_branch_name("my branch"));
        assert!(!validate_branch_name("main\t"));
        assert!(!validate_branch_name("main\ndev"));
        assert!(!validate_branch_name("main~1"));
        assert!(!validate_branch_name("branch^2"));
        assert!(!validate_branch_name("branch:name"));
        assert!(!validate_branch_name("branch?"));
        assert!(!validate_branch_name("branch*"));
        assert!(!validate_branch_name("branch[0]"));
        assert!(!validate_branch_name("branch\\foo"));
        assert!(!validate_branch_name("@{upstream}"));
        assert!(!validate_branch_name(".hidden"));
        assert!(!validate_branch_name("trailing."));
        assert!(!validate_branch_name("main.lock"));
    }

    #[test]
    fn validate_branch_name_accepts_valid() {
        assert!(validate_branch_name("main"));
        assert!(validate_branch_name("develop"));
        assert!(validate_branch_name("feature/foo"));
        assert!(validate_branch_name("release-1.0"));
        assert!(validate_branch_name("v2.0.0"));
        assert!(validate_branch_name("user@feature"));
    }

    #[test]
    fn parse_full_config_with_hooks_and_local() {
        let toml_str = r#"
max_reviews = 5

[vcs]
mode = "local"
target_branch = "develop"
merge_strategy = "rebase"

[hooks]
post_merge = ["git push"]
"#;
        let f: ProjectConfig = toml::from_str(toml_str).unwrap();
        let vcs = f.vcs.unwrap();
        assert_eq!(vcs.mode.unwrap(), VcsMode::Local);
        assert_eq!(vcs.target_branch.unwrap(), "develop");
        assert_eq!(vcs.merge_strategy.unwrap(), MergeStrategy::Rebase);
        assert_eq!(f.hooks.unwrap().post_merge.unwrap(), vec!["git push"]);
        assert_eq!(f.max_reviews.unwrap(), 5);
    }
}
