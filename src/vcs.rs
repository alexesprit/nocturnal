use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::util::retry;
use crate::project_config::VcsMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    GitHub,
    GitLab,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::GitHub => write!(f, "github"),
            Platform::GitLab => write!(f, "gitlab"),
        }
    }
}

pub fn detect_platform(project_root: &str, vcs_mode: VcsMode) -> Option<Platform> {
    match vcs_mode {
        VcsMode::Off => return None,
        VcsMode::GitHub => return Some(Platform::GitHub),
        VcsMode::GitLab => return Some(Platform::GitLab),
        VcsMode::Auto => {}
    }

    let url = crate::git::remote_url(project_root)?;
    if url.contains("gitlab") {
        Some(Platform::GitLab)
    } else if url.contains("github") {
        Some(Platform::GitHub)
    } else {
        None
    }
}

pub struct Proposal {
    pub id: String,
    pub url: String,
}

pub fn create_proposal(
    platform: Platform,
    wt_path: &str,
    title: &str,
    description: &str,
) -> Result<Proposal> {
    retry("VCS", || match platform {
        Platform::GitLab => {
            let output = Command::new("glab")
                .args([
                    "mr",
                    "create",
                    "--title",
                    title,
                    "--description",
                    description,
                    "--target-branch",
                    "main",
                    "--label",
                    "nocturnal",
                    "--yes",
                ])
                .current_dir(wt_path)
                .output()
                .context("Failed to create GitLab MR")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("Failed to create GitLab MR: {}", stderr.trim());
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let url = stdout
                .split_whitespace()
                .find(|s| s.starts_with("https://"))
                .or_else(|| {
                    stderr
                        .split_whitespace()
                        .find(|s| s.starts_with("https://"))
                })
                .unwrap_or("")
                .to_string();

            let id = extract_trailing_number(&url)?;
            Ok(Proposal { id, url })
        }
        Platform::GitHub => {
            let output = Command::new("gh")
                .args([
                    "pr",
                    "create",
                    "--title",
                    title,
                    "--body",
                    description,
                    "--base",
                    "main",
                    "--label",
                    "nocturnal",
                ])
                .current_dir(wt_path)
                .output()
                .context("Failed to create GitHub PR")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("Failed to create GitHub PR: {}", stderr.trim());
            }

            let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let id = extract_trailing_number(&url)?;
            Ok(Proposal { id, url })
        }
    })
}

pub fn delete_remote_branch(wt_path: &str, branch: &str) -> bool {
    retry("VCS", || {
        let status = Command::new("git")
            .args(["push", "origin", "--delete", branch])
            .current_dir(wt_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => bail!("git push --delete exited with status {s}"),
            Err(e) => Err(anyhow::Error::from(e)),
        }
    })
    .is_ok()
}

pub fn enable_auto_merge(platform: Platform, wt_path: &str, proposal_id: &str) -> bool {
    retry("VCS", || {
        let status = match platform {
            Platform::GitLab => Command::new("glab")
                .args(["mr", "merge", proposal_id, "--auto", "--yes"])
                .current_dir(wt_path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status(),
            Platform::GitHub => Command::new("gh")
                .args(["pr", "merge", proposal_id, "--auto", "--squash"])
                .current_dir(wt_path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status(),
        };
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => bail!("auto-merge command exited with status {s}"),
            Err(e) => Err(anyhow::Error::from(e)),
        }
    })
    .is_ok()
}

#[derive(Debug)]
pub enum ProposalState {
    Open,
    Merged,
    Closed,
}

pub fn get_proposal_state(
    platform: Platform,
    wt_path: &str,
    proposal_id: &str,
) -> Result<ProposalState> {
    retry("VCS", || {
        let state_str = match platform {
            Platform::GitLab => {
                let output = Command::new("glab")
                    .args(["mr", "view", proposal_id, "-F", "json"])
                    .current_dir(wt_path)
                    .output()
                    .context("Failed to view GitLab MR")?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    bail!("Failed to view GitLab MR #{proposal_id}: {}", stderr.trim());
                }
                let json: serde_json::Value =
                    serde_json::from_slice(&output.stdout).context("Failed to parse MR JSON")?;
                json["state"].as_str().unwrap_or("unknown").to_string()
            }
            Platform::GitHub => {
                let output = Command::new("gh")
                    .args(["pr", "view", proposal_id, "--json", "state"])
                    .current_dir(wt_path)
                    .output()
                    .context("Failed to view GitHub PR")?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    bail!("Failed to view GitHub PR #{proposal_id}: {}", stderr.trim());
                }
                let json: serde_json::Value =
                    serde_json::from_slice(&output.stdout).context("Failed to parse PR JSON")?;
                json["state"].as_str().unwrap_or("unknown").to_string()
            }
        };

        match state_str.to_lowercase().as_str() {
            "merged" => Ok(ProposalState::Merged),
            "closed" => Ok(ProposalState::Closed),
            _ => Ok(ProposalState::Open),
        }
    })
}

pub fn fetch_unresolved_comments(
    platform: Platform,
    wt_path: &str,
    proposal_id: &str,
) -> Result<String> {
    retry("VCS", || match platform {
        Platform::GitLab => {
            let output = Command::new("glab")
                .args([
                    "api",
                    &format!("projects/:fullpath/merge_requests/{proposal_id}/discussions"),
                ])
                .current_dir(wt_path)
                .output()
                .context("Failed to fetch GitLab discussions")?;

            const EMPTY: &[serde_json::Value] = &[];
            let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
            let comments: Vec<serde_json::Value> = json
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(EMPTY)
                .iter()
                .flat_map(|d| {
                    d["notes"]
                        .as_array()
                        .map(Vec::as_slice)
                        .unwrap_or(EMPTY)
                        .iter()
                        .filter(|n| n["resolved"] == false)
                        .map(|n| {
                            serde_json::json!({
                                "id": n["id"],
                                "author": n["author"]["username"],
                                "body": n["body"],
                                "path": n["position"]["new_path"],
                                "line": n["position"]["new_line"],
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            Ok(serde_json::to_string_pretty(&comments)?)
        }
        Platform::GitHub => {
            let (owner, repo) = gh_owner_repo(wt_path)?;

            let query = r#"query($owner: String!, $repo: String!, $pr: Int!) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $pr) {
      reviewThreads(first: 100) {
        nodes {
          id
          isResolved
          comments(first: 1) {
            nodes {
              databaseId
              body
              path
              line
              author { login }
            }
          }
        }
      }
    }
  }
}"#;

            let output = Command::new("gh")
                .args([
                    "api",
                    "graphql",
                    "-f",
                    &format!("query={query}"),
                    "-F",
                    &format!("owner={owner}"),
                    "-F",
                    &format!("repo={repo}"),
                    "-F",
                    &format!("pr={proposal_id}"),
                ])
                .current_dir(wt_path)
                .output()
                .context("Failed to fetch GitHub review threads")?;

            let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
            let empty = vec![];
            let threads = json["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"]
                .as_array()
                .unwrap_or(&empty);

            let mut comments: Vec<serde_json::Value> = threads
                .iter()
                .filter(|t| t["isResolved"] == false)
                .filter_map(|t| {
                    let thread_id = t["id"].as_str()?;
                    let c = t["comments"]["nodes"].as_array()?.first()?;
                    Some(serde_json::json!({
                        "id": c["databaseId"],
                        "thread_id": thread_id,
                        "author": c["author"]["login"],
                        "body": c["body"],
                        "path": c["path"],
                        "line": c["line"],
                    }))
                })
                .collect();

            // Also fetch general PR (issue-level) comments
            let issue_output = Command::new("gh")
                .args([
                    "api",
                    &format!("repos/{owner}/{repo}/issues/{proposal_id}/comments"),
                ])
                .current_dir(wt_path)
                .output()
                .context("Failed to fetch GitHub issue comments")?;

            if issue_output.status.success() {
                if let Ok(serde_json::Value::Array(issue_comments)) =
                    serde_json::from_slice::<serde_json::Value>(&issue_output.stdout)
                {
                    for c in issue_comments {
                        comments.push(serde_json::json!({
                            "id": c["id"],
                            "thread_id": null,
                            "author": c["user"]["login"],
                            "body": c["body"],
                            "path": null,
                            "line": null,
                        }));
                    }
                }
            }

            Ok(serde_json::to_string_pretty(&comments)?)
        }
    })
}

fn gh_owner_repo(wt_path: &str) -> Result<(String, String)> {
    let output = Command::new("gh")
        .args(["repo", "view", "--json", "owner,name"])
        .current_dir(wt_path)
        .output()
        .context("Failed to get GitHub repo info")?;
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse gh repo view output")?;
    let owner = json["owner"]["login"]
        .as_str()
        .context("Missing owner in gh repo view")?
        .to_string();
    let repo = json["name"]
        .as_str()
        .context("Missing repo name in gh repo view")?
        .to_string();
    Ok((owner, repo))
}

fn extract_trailing_number(s: &str) -> Result<String> {
    let id: String = s
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if id.is_empty() {
        bail!("Could not extract proposal ID from: {s}");
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- extract_trailing_number ---

    #[test]
    fn extract_trailing_number_from_mr_url() {
        let id =
            extract_trailing_number("https://gitlab.com/org/repo/-/merge_requests/42").unwrap();
        assert_eq!(id, "42");
    }

    #[test]
    fn extract_trailing_number_from_pr_url() {
        let id = extract_trailing_number("https://github.com/org/repo/pull/123").unwrap();
        assert_eq!(id, "123");
    }

    #[test]
    fn extract_trailing_number_single_digit() {
        assert_eq!(extract_trailing_number("url/7").unwrap(), "7");
    }

    #[test]
    fn extract_trailing_number_no_digits() {
        assert!(extract_trailing_number("no-digits-here").is_err());
    }

    #[test]
    fn extract_trailing_number_empty_string() {
        assert!(extract_trailing_number("").is_err());
    }

    #[test]
    fn extract_trailing_number_digits_in_middle_only() {
        // "abc123def" — no trailing digits
        assert!(extract_trailing_number("abc123def").is_err());
    }

    // --- detect_platform (mode-based, no git calls) ---

    #[test]
    fn detect_platform_off() {
        assert!(detect_platform("/unused", VcsMode::Off).is_none());
    }

    #[test]
    fn detect_platform_forced_github() {
        assert_eq!(
            detect_platform("/unused", VcsMode::GitHub).unwrap(),
            Platform::GitHub
        );
    }

    #[test]
    fn detect_platform_forced_gitlab() {
        assert_eq!(
            detect_platform("/unused", VcsMode::GitLab).unwrap(),
            Platform::GitLab
        );
    }

    // --- Platform display ---

    #[test]
    fn platform_display() {
        assert_eq!(Platform::GitHub.to_string(), "github");
        assert_eq!(Platform::GitLab.to_string(), "gitlab");
    }
}
