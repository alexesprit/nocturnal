use std::io;
use std::path::Path;

use serde::Deserialize;

pub const DEFAULT_MAX_REVIEWS: u32 = 3;
pub const DEFAULT_MAX_BUDGET: Option<u32> = None;
pub const DEFAULT_MODEL: &str = "sonnet";
pub const DEFAULT_CODEX_MODEL: &str = "gpt-5.4";
pub const DEFAULT_CODEX_REASONING_EFFORT: &str = "high";
pub const DEFAULT_TARGET_BRANCH: &str = "main";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Claude,
    Codex,
}

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
    base_branch: Option<String>,
    target_branch: Option<String>,
    merge_strategy: Option<MergeStrategy>,
}

#[derive(Debug, Default, Deserialize)]
struct HooksConfig {
    pre_merge: Option<Vec<String>>,
    post_merge: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct ClaudeConfig {
    model: Option<String>,
    implement_model: Option<String>,
    review_model: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct CodexConfig {
    model: Option<String>,
    implement_model: Option<String>,
    review_model: Option<String>,
    reasoning_effort: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectConfig {
    vcs: Option<VcsConfig>,
    hooks: Option<HooksConfig>,
    max_reviews: Option<u32>,
    max_budget: Option<u32>,
    auto_develop: Option<bool>,
    provider: Option<Provider>,
    implement_provider: Option<Provider>,
    review_provider: Option<Provider>,
    claude: Option<ClaudeConfig>,
    codex: Option<CodexConfig>,
}

#[derive(Clone)]
pub struct ProjectSettings {
    pub vcs_mode: VcsMode,
    pub auto_merge: bool,
    pub delete_branch_on_merge: bool,
    pub base_branch: String,
    pub target_branch: String,
    pub merge_strategy: MergeStrategy,
    pub max_reviews: u32,
    pub max_budget: Option<u32>,
    pub auto_develop: bool,
    #[allow(dead_code)]
    pub provider: Provider,
    pub implement_provider: Provider,
    pub review_provider: Provider,
    pub implement_model: String,
    pub review_model: String,
    pub codex_reasoning_effort: String,
    pub pre_merge_hooks: Vec<String>,
    pub post_merge_hooks: Vec<String>,
}

fn resolve_model(
    provider: Provider,
    claude: Option<&ClaudeConfig>,
    codex: Option<&CodexConfig>,
    claude_picker: impl Fn(&ClaudeConfig) -> &Option<String>,
    codex_picker: impl Fn(&CodexConfig) -> &Option<String>,
) -> String {
    match provider {
        Provider::Codex => {
            let default = codex
                .and_then(|c| c.model.as_deref())
                .unwrap_or(DEFAULT_CODEX_MODEL);
            codex
                .and_then(|c| codex_picker(c).clone())
                .unwrap_or_else(|| default.to_string())
        }
        Provider::Claude => {
            let default = claude
                .and_then(|c| c.model.as_deref())
                .unwrap_or(DEFAULT_MODEL);
            claude
                .and_then(|c| claude_picker(c).clone())
                .unwrap_or_else(|| default.to_string())
        }
    }
}

fn resolve_merge_strategy(vcs: &VcsConfig) -> MergeStrategy {
    vcs.merge_strategy
        .unwrap_or_else(|| match vcs.mode.unwrap_or_default() {
            VcsMode::Local => MergeStrategy::Rebase,
            _ => MergeStrategy::Ff,
        })
}

pub fn load_project_settings(project_root: &Path) -> ProjectSettings {
    let path = project_root.join(".nocturnal.toml");
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
            let hooks = f.hooks.unwrap_or_default();
            let merge_strategy = resolve_merge_strategy(&vcs);
            let provider = f.provider.unwrap_or_default();
            let implement_provider = f.implement_provider.unwrap_or(provider);
            let review_provider = f.review_provider.unwrap_or(provider);

            let implement_model = resolve_model(
                implement_provider,
                f.claude.as_ref(),
                f.codex.as_ref(),
                |c| &c.implement_model,
                |c| &c.implement_model,
            );
            let review_model = resolve_model(
                review_provider,
                f.claude.as_ref(),
                f.codex.as_ref(),
                |c| &c.review_model,
                |c| &c.review_model,
            );
            let codex_reasoning_effort = f
                .codex
                .as_ref()
                .and_then(|c| c.reasoning_effort.clone())
                .unwrap_or_else(|| DEFAULT_CODEX_REASONING_EFFORT.to_string());

            let base_branch = vcs
                .base_branch
                .filter(|b| {
                    if validate_branch_name(b) {
                        true
                    } else {
                        tracing::warn!("Invalid base_branch {b:?}, using default");
                        false
                    }
                })
                .unwrap_or_else(|| DEFAULT_TARGET_BRANCH.to_string());
            ProjectSettings {
                vcs_mode: vcs.mode.unwrap_or_default(),
                auto_merge: vcs.auto_merge.unwrap_or(true),
                delete_branch_on_merge: vcs.delete_branch_on_merge.unwrap_or(false),
                base_branch: base_branch.clone(),
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
                    .unwrap_or(base_branch),
                merge_strategy,
                max_reviews: f.max_reviews.unwrap_or(DEFAULT_MAX_REVIEWS),
                max_budget: f.max_budget.or(DEFAULT_MAX_BUDGET),
                auto_develop: f.auto_develop.unwrap_or(true),
                provider,
                implement_provider,
                review_provider,
                implement_model,
                review_model,
                codex_reasoning_effort,
                pre_merge_hooks: hooks.pre_merge.unwrap_or_default(),
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
            base_branch: DEFAULT_TARGET_BRANCH.to_string(),
            target_branch: DEFAULT_TARGET_BRANCH.to_string(),
            merge_strategy: MergeStrategy::default(),
            max_reviews: DEFAULT_MAX_REVIEWS,
            max_budget: DEFAULT_MAX_BUDGET,
            auto_develop: true,
            provider: Provider::default(),
            implement_provider: Provider::default(),
            review_provider: Provider::default(),
            implement_model: DEFAULT_MODEL.to_string(),
            review_model: DEFAULT_MODEL.to_string(),
            codex_reasoning_effort: DEFAULT_CODEX_REASONING_EFFORT.to_string(),
            pre_merge_hooks: Vec::new(),
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
        && !Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lock"))
        && !name.contains(|c: char| {
            c.is_whitespace()
                || c.is_control()
                || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        })
}

/// Convenience wrapper kept for callers that only need vcs mode.
pub fn load_vcs_mode(project_root: &Path) -> VcsMode {
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
        assert_eq!(load_vcs_mode(Path::new("/nonexistent/path")), VcsMode::Off);
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
        let settings = load_project_settings(Path::new("/nonexistent/path"));
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
        let settings = load_project_settings(Path::new("/nonexistent/path"));
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
        let settings = load_project_settings(Path::new("/nonexistent/path"));
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
        let settings = load_project_settings(Path::new("/nonexistent/path"));
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
        let settings = load_project_settings(Path::new("/nonexistent/path"));
        assert_eq!(settings.merge_strategy, MergeStrategy::Ff);
    }

    #[test]
    fn local_mode_defaults_to_rebase() {
        let f: ProjectConfig = toml::from_str("[vcs]\nmode = \"local\"").unwrap();
        let vcs = f.vcs.unwrap();
        assert_eq!(resolve_merge_strategy(&vcs), MergeStrategy::Rebase);
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
        let settings = load_project_settings(Path::new("/nonexistent/path"));
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
        let settings = load_project_settings(Path::new("/nonexistent/path"));
        assert!(settings.post_merge_hooks.is_empty());
    }

    #[test]
    fn parse_pre_merge_hooks() {
        let f: ProjectConfig =
            toml::from_str("[hooks]\npre_merge = [\"cargo test\", \"cargo clippy\"]").unwrap();
        let hooks = f.hooks.unwrap();
        assert_eq!(hooks.pre_merge.unwrap(), vec!["cargo test", "cargo clippy"]);
    }

    #[test]
    fn parse_empty_pre_merge() {
        let f: ProjectConfig = toml::from_str("[hooks]").unwrap();
        let hooks = f.hooks.unwrap();
        assert!(hooks.pre_merge.is_none());
    }

    #[test]
    fn parse_pre_merge_empty_list() {
        let f: ProjectConfig = toml::from_str("[hooks]\npre_merge = []").unwrap();
        let hooks = f.hooks.unwrap();
        assert!(hooks.pre_merge.unwrap().is_empty());
    }

    #[test]
    fn pre_merge_hooks_default_to_empty() {
        let settings = load_project_settings(Path::new("/nonexistent/path"));
        assert!(settings.pre_merge_hooks.is_empty());
    }

    #[test]
    fn parse_both_hooks() {
        let f: ProjectConfig =
            toml::from_str("[hooks]\npre_merge = [\"cargo test\"]\npost_merge = [\"git push\"]")
                .unwrap();
        let hooks = f.hooks.unwrap();
        assert_eq!(hooks.pre_merge.unwrap(), vec!["cargo test"]);
        assert_eq!(hooks.post_merge.unwrap(), vec!["git push"]);
    }

    #[test]
    fn auto_develop_defaults_to_true() {
        let settings = load_project_settings(Path::new("/nonexistent/path"));
        assert!(settings.auto_develop);
    }

    #[test]
    fn parse_auto_develop_false() {
        let f: ProjectConfig = toml::from_str("auto_develop = false").unwrap();
        assert!(!f.auto_develop.unwrap());
    }

    #[test]
    fn load_settings_auto_develop_false() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".nocturnal.toml");
        let mut f = std::fs::File::create(&toml_path).unwrap();
        write!(f, "auto_develop = false\n").unwrap();
        let settings = load_project_settings(dir.path());
        assert!(!settings.auto_develop);
    }

    // --- Per-action provider ---

    #[test]
    fn implement_provider_codex_review_provider_claude_mixed() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".nocturnal.toml");
        let mut f = std::fs::File::create(&toml_path).unwrap();
        write!(
            f,
            concat!(
                "provider = \"claude\"\n",
                "implement_provider = \"codex\"\n",
                "[codex]\nmodel = \"o3\"\nimplement_model = \"o4-mini\"\n",
                "[claude]\nmodel = \"sonnet\"\nreview_model = \"haiku\"\n",
            )
        )
        .unwrap();
        let settings = load_project_settings(dir.path());
        assert_eq!(settings.implement_provider, Provider::Codex);
        assert_eq!(settings.review_provider, Provider::Claude);
        assert_eq!(settings.implement_model, "o4-mini");
        assert_eq!(settings.review_model, "haiku");
    }

    #[test]
    fn only_implement_provider_set_review_falls_back_to_provider() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".nocturnal.toml");
        let mut f = std::fs::File::create(&toml_path).unwrap();
        write!(
            f,
            concat!(
                "provider = \"claude\"\n",
                "implement_provider = \"codex\"\n",
                "[codex]\nmodel = \"o3\"\n",
                "[claude]\nmodel = \"opus\"\n",
            )
        )
        .unwrap();
        let settings = load_project_settings(dir.path());
        assert_eq!(settings.implement_provider, Provider::Codex);
        assert_eq!(settings.review_provider, Provider::Claude);
        assert_eq!(settings.implement_model, "o3");
        assert_eq!(settings.review_model, "opus");
    }

    #[test]
    fn neither_per_action_provider_falls_back_to_provider() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".nocturnal.toml");
        let mut f = std::fs::File::create(&toml_path).unwrap();
        write!(f, "provider = \"codex\"\n[codex]\nmodel = \"o3\"\n").unwrap();
        let settings = load_project_settings(dir.path());
        assert_eq!(settings.implement_provider, Provider::Codex);
        assert_eq!(settings.review_provider, Provider::Codex);
        assert_eq!(settings.implement_model, "o3");
        assert_eq!(settings.review_model, "o3");
    }

    #[test]
    fn both_per_action_providers_same_value() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".nocturnal.toml");
        let mut f = std::fs::File::create(&toml_path).unwrap();
        write!(
            f,
            concat!(
                "provider = \"claude\"\n",
                "implement_provider = \"claude\"\n",
                "review_provider = \"claude\"\n",
                "[claude]\nmodel = \"opus\"\n",
            )
        )
        .unwrap();
        let settings = load_project_settings(dir.path());
        assert_eq!(settings.implement_provider, Provider::Claude);
        assert_eq!(settings.review_provider, Provider::Claude);
        assert_eq!(settings.implement_model, "opus");
        assert_eq!(settings.review_model, "opus");
    }

    #[test]
    fn per_action_providers_default_to_claude() {
        let settings = load_project_settings(Path::new("/nonexistent/path"));
        assert_eq!(settings.implement_provider, Provider::Claude);
        assert_eq!(settings.review_provider, Provider::Claude);
    }

    #[test]
    fn parse_implement_provider_and_review_provider() {
        let f: ProjectConfig =
            toml::from_str("implement_provider = \"codex\"\nreview_provider = \"claude\"").unwrap();
        assert_eq!(f.implement_provider.unwrap(), Provider::Codex);
        assert_eq!(f.review_provider.unwrap(), Provider::Claude);
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

    // --- Provider ---

    #[test]
    fn provider_defaults_to_claude() {
        let f: ProjectConfig = toml::from_str("").unwrap();
        assert_eq!(f.provider.unwrap_or_default(), Provider::Claude);
    }

    #[test]
    fn parse_provider_claude_explicit() {
        let f: ProjectConfig = toml::from_str("provider = \"claude\"").unwrap();
        assert_eq!(f.provider.unwrap(), Provider::Claude);
    }

    #[test]
    fn parse_provider_codex() {
        let f: ProjectConfig = toml::from_str("provider = \"codex\"").unwrap();
        assert_eq!(f.provider.unwrap(), Provider::Codex);
    }

    #[test]
    fn parse_unrecognized_provider_is_error() {
        assert!(toml::from_str::<ProjectConfig>("provider = \"openai\"").is_err());
    }

    #[test]
    fn codex_section_parses_all_fields() {
        let f: ProjectConfig = toml::from_str(
            "provider = \"codex\"\n[codex]\nmodel = \"o3\"\nimplement_model = \"o4-mini\"\nreview_model = \"o3-mini\"",
        )
        .unwrap();
        let codex = f.codex.unwrap();
        assert_eq!(codex.model.as_deref(), Some("o3"));
        assert_eq!(codex.implement_model.as_deref(), Some("o4-mini"));
        assert_eq!(codex.review_model.as_deref(), Some("o3-mini"));
    }

    #[test]
    fn codex_model_fallback_to_model() {
        let f: ProjectConfig =
            toml::from_str("provider = \"codex\"\n[codex]\nmodel = \"o3\"").unwrap();
        let codex = f.codex.unwrap();
        let default_model = codex.model.as_deref().unwrap_or(DEFAULT_CODEX_MODEL);
        let implement_model = codex
            .implement_model
            .unwrap_or_else(|| default_model.to_string());
        assert_eq!(implement_model, "o3");
    }

    #[test]
    fn codex_model_fallback_to_default_constant() {
        let f: ProjectConfig = toml::from_str("provider = \"codex\"\n[codex]").unwrap();
        let codex = f.codex.unwrap();
        let default_model = codex.model.as_deref().unwrap_or(DEFAULT_CODEX_MODEL);
        assert_eq!(default_model, DEFAULT_CODEX_MODEL);
        let implement_model = codex
            .implement_model
            .unwrap_or_else(|| default_model.to_string());
        assert_eq!(implement_model, DEFAULT_CODEX_MODEL);
    }

    #[test]
    fn load_settings_with_codex_provider() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".nocturnal.toml");
        let mut f = std::fs::File::create(&toml_path).unwrap();
        write!(
            f,
            "provider = \"codex\"\n[codex]\nmodel = \"o3\"\nimplement_model = \"o4-mini\"\n"
        )
        .unwrap();
        let settings = load_project_settings(dir.path());
        assert_eq!(settings.provider, Provider::Codex);
        assert_eq!(settings.implement_model, "o4-mini");
        assert_eq!(settings.review_model, "o3");
    }

    #[test]
    fn load_settings_with_claude_provider_explicit() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".nocturnal.toml");
        let mut f = std::fs::File::create(&toml_path).unwrap();
        write!(
            f,
            "provider = \"claude\"\n[claude]\nmodel = \"opus\"\nreview_model = \"haiku\"\n"
        )
        .unwrap();
        let settings = load_project_settings(dir.path());
        assert_eq!(settings.provider, Provider::Claude);
        assert_eq!(settings.implement_model, "opus");
        assert_eq!(settings.review_model, "haiku");
    }

    #[test]
    fn load_settings_missing_provider_defaults_to_claude() {
        let settings = load_project_settings(Path::new("/nonexistent/path"));
        assert_eq!(settings.provider, Provider::Claude);
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
pre_merge = ["cargo test"]
post_merge = ["git push"]
"#;
        let f: ProjectConfig = toml::from_str(toml_str).unwrap();
        let vcs = f.vcs.unwrap();
        assert_eq!(vcs.mode.unwrap(), VcsMode::Local);
        assert_eq!(vcs.target_branch.unwrap(), "develop");
        assert_eq!(vcs.merge_strategy.unwrap(), MergeStrategy::Rebase);
        let hooks = f.hooks.unwrap();
        assert_eq!(hooks.pre_merge.unwrap(), vec!["cargo test"]);
        assert_eq!(hooks.post_merge.unwrap(), vec!["git push"]);
        assert_eq!(f.max_reviews.unwrap(), 5);
    }
}
