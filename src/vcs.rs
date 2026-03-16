use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::config::VcsPlatformOverride;

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

pub fn detect_platform(
    project_root: &str,
    override_: &Option<VcsPlatformOverride>,
) -> Option<Platform> {
    match override_ {
        Some(VcsPlatformOverride::Disabled) => return None,
        Some(VcsPlatformOverride::Forced(p)) => {
            return match p.as_str() {
                "github" => Some(Platform::GitHub),
                "gitlab" => Some(Platform::GitLab),
                _ => None,
            };
        }
        None => {}
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
    match platform {
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

            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );

            let url = combined
                .split_whitespace()
                .find(|s| s.starts_with("https://"))
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
    }
}

pub fn enable_auto_merge(platform: Platform, wt_path: &str, proposal_id: &str) -> bool {
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
    status.is_ok_and(|s| s.success())
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
}

pub fn fetch_unresolved_comments(
    platform: Platform,
    wt_path: &str,
    proposal_id: &str,
) -> Result<String> {
    match platform {
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
            let inline_output = Command::new("gh")
                .args([
                    "api",
                    &format!("repos/{{owner}}/{{repo}}/pulls/{proposal_id}/comments"),
                ])
                .current_dir(wt_path)
                .output()
                .context("Failed to fetch GitHub inline comments")?;

            let general_output = Command::new("gh")
                .args([
                    "api",
                    &format!("repos/{{owner}}/{{repo}}/issues/{proposal_id}/comments"),
                ])
                .current_dir(wt_path)
                .output()
                .context("Failed to fetch GitHub general comments")?;

            let inline: Vec<serde_json::Value> =
                serde_json::from_slice(&inline_output.stdout).unwrap_or_default();
            let general: Vec<serde_json::Value> =
                serde_json::from_slice(&general_output.stdout).unwrap_or_default();

            let mut comments: Vec<serde_json::Value> = inline
                .iter()
                .filter(|c| !c["position"].is_null())
                .map(|c| {
                    serde_json::json!({
                        "id": c["id"],
                        "author": c["user"]["login"],
                        "body": c["body"],
                        "path": c["path"],
                        "line": c["position"],
                    })
                })
                .collect();

            comments.extend(general.iter().map(|c| {
                serde_json::json!({
                    "id": c["id"],
                    "author": c["user"]["login"],
                    "body": c["body"],
                    "path": null,
                    "line": null,
                })
            }));

            Ok(serde_json::to_string_pretty(&comments)?)
        }
    }
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
