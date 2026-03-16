use std::io;
use std::path::Path;

use serde::Deserialize;

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
}

pub fn load_vcs_mode(project_root: &str) -> VcsMode {
    let path = Path::new(project_root).join(".nocturnal.toml");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return VcsMode::default(),
        Err(e) => {
            tracing::warn!("Failed to read {}: {e}", path.display());
            return VcsMode::default();
        }
    };
    match toml::from_str::<ProjectConfig>(&content) {
        Ok(f) => f.vcs.unwrap_or_default(),
        Err(e) => {
            tracing::warn!("Failed to parse {}: {e}", path.display());
            VcsMode::default()
        }
    }
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
}
