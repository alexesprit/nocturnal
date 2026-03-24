use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::util::retry;

const WORKTREE_PREFIX: &str = "nocturnal";

pub fn worktree_branch(task_id: &str) -> String {
    debug_assert!(
        crate::td::validate_task_id(task_id).is_ok(),
        "task_id must be validated before constructing branch name: {task_id:?}"
    );
    format!("{WORKTREE_PREFIX}/{task_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_branch_format() {
        assert_eq!(worktree_branch("task-42"), "nocturnal/task-42");
    }
}

/// Parse `git worktree list --porcelain` and return `(worktree_path, task_id)` pairs
/// for all worktrees on branches matching `refs/heads/nocturnal/<task_id>`.
pub fn list_nocturnal_worktrees(project_root: &str) -> Result<Vec<(String, String)>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(project_root)
        .output()
        .context("Failed to list worktrees")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut result = Vec::new();
    let mut current_path: Option<String> = None;

    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path.to_string());
        } else if let Some(task_id) = line.strip_prefix("branch refs/heads/nocturnal/") {
            if let Some(path) = current_path.take() {
                result.push((path, task_id.to_string()));
            }
        }
    }

    Ok(result)
}

pub fn worktree_path(project_root: &str, task_id: &str) -> Result<Option<String>> {
    let worktrees = list_nocturnal_worktrees(project_root)?;
    Ok(worktrees
        .into_iter()
        .find(|(_, id)| id == task_id)
        .map(|(path, _)| path))
}

fn fetch_branch(project_root: &str, branch: &str) {
    let status = Command::new("git")
        .args(["fetch", "origin", branch])
        .current_dir(project_root)
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            tracing::warn!("git fetch origin {branch} exited with {s}; continuing with local state")
        }
        Err(e) => {
            tracing::warn!("git fetch origin {branch} failed: {e}; continuing with local state")
        }
    }
}

pub fn ensure_worktree(project_root: &str, task_id: &str, base_branch: &str) -> Result<String> {
    if let Some(path) = worktree_path(project_root, task_id)? {
        return Ok(path);
    }

    fetch_branch(project_root, base_branch);

    let branch = worktree_branch(task_id);
    tracing::info!("Creating worktree: {branch}");

    let status = Command::new("git")
        .args([
            "gtr",
            "new",
            &branch,
            "--from",
            &format!("origin/{base_branch}"),
        ])
        .current_dir(project_root)
        .status()
        .context("Failed to run git gtr new")?;

    if !status.success() {
        bail!("git gtr new {branch} failed");
    }

    worktree_path(project_root, task_id)?
        .ok_or_else(|| anyhow::anyhow!("Worktree not found after creation"))
}

/// Check if `potential_ancestor` is an ancestor of `branch`.
pub fn is_ancestor(project_root: &str, potential_ancestor: &str, branch: &str) -> Result<bool> {
    let status = Command::new("git")
        .args(["merge-base", "--is-ancestor", potential_ancestor, branch])
        .current_dir(project_root)
        .status()
        .context("Failed to run git merge-base --is-ancestor")?;
    Ok(status.success())
}

/// Fast-forward merge: atomically update `target_branch` ref to `source_branch` without checkout.
/// If `target_branch` is currently checked out, also updates the working tree.
pub fn merge_ff_only(project_root: &str, target_branch: &str, source_branch: &str) -> Result<()> {
    let checked_out = current_branch(project_root)
        .map(|b| b == target_branch)
        .unwrap_or(false);

    if checked_out {
        // Target is checked out — use git merge --ff-only to keep working tree in sync
        let output = Command::new("git")
            .args(["merge", "--ff-only", source_branch])
            .current_dir(project_root)
            .output()
            .context("Failed to run git merge --ff-only")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Fast-forward merge of {source_branch} into {target_branch} failed: {}",
                stderr.trim()
            );
        }
    } else {
        // Verify ff-only semantics: target must be an ancestor of source
        if !is_ancestor(project_root, target_branch, source_branch)? {
            bail!(
                "Fast-forward merge not possible: {target_branch} is not an ancestor of {source_branch}"
            );
        }
        // Target is not checked out — update ref atomically without touching working tree
        let refspec = format!("{source_branch}:{target_branch}");
        let output = Command::new("git")
            .args(["fetch", ".", &refspec])
            .current_dir(project_root)
            .output()
            .context("Failed to run git fetch for ff merge")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Fast-forward merge of {source_branch} into {target_branch} failed \
                (target may be checked out in another worktree): {}",
                stderr.trim()
            );
        }
    }
    Ok(())
}

/// Returns the current branch name, or None if HEAD is detached.
fn current_branch(project_root: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(project_root)
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() || name == "HEAD" {
        None
    } else {
        Some(name)
    }
}

/// Merge with a merge commit. Requires a clean working tree in the project root.
/// Uses a Drop guard to restore the original branch on error/panic.
pub fn merge_no_ff(project_root: &str, target_branch: &str, source_branch: &str) -> Result<()> {
    // Check working tree is clean
    let status_output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(project_root)
        .output()
        .context("Failed to check git status")?;
    let porcelain = String::from_utf8_lossy(&status_output.stdout);
    if !porcelain.trim().is_empty() {
        bail!("Working tree is not clean in {project_root} — cannot perform no-ff merge");
    }

    // Record original branch (bail on detached HEAD)
    let original_branch = current_branch(project_root)
        .ok_or_else(|| anyhow::anyhow!("Cannot perform no-ff merge from detached HEAD state"))?;

    // Drop guard to restore original branch on error/panic
    struct BranchGuard<'a> {
        project_root: &'a str,
        original_branch: String,
        armed: bool,
    }
    impl Drop for BranchGuard<'_> {
        fn drop(&mut self) {
            if self.armed {
                Command::new("git")
                    .args(["checkout", &self.original_branch])
                    .current_dir(self.project_root)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .ok();
            }
        }
    }

    let mut guard = BranchGuard {
        project_root,
        original_branch: original_branch.clone(),
        armed: true,
    };

    // Checkout target
    let status = Command::new("git")
        .args(["checkout", target_branch])
        .current_dir(project_root)
        .status()
        .context("Failed to checkout target branch")?;
    if !status.success() {
        bail!("Failed to checkout {target_branch}");
    }

    // Merge --no-ff
    let msg = format!("Merge {source_branch} into {target_branch}");
    let output = Command::new("git")
        .args(["merge", "--no-ff", source_branch, "-m", &msg])
        .current_dir(project_root)
        .output()
        .context("Failed to run git merge --no-ff")?;

    if !output.status.success() {
        // Abort merge if in progress
        Command::new("git")
            .args(["merge", "--abort"])
            .current_dir(project_root)
            .status()
            .ok();
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "No-ff merge of {source_branch} into {target_branch} failed: {}",
            stderr.trim()
        );
    }

    // Guard restores original branch on drop (both success and error)
    guard.armed = original_branch != target_branch;
    Ok(())
}

/// Rebase source branch onto target, then fast-forward target to source.
pub fn rebase_and_merge(
    project_root: &str,
    target_branch: &str,
    source_branch: &str,
    wt_path: &str,
) -> Result<()> {
    // Rebase in the worktree
    let output = Command::new("git")
        .args(["rebase", target_branch])
        .current_dir(wt_path)
        .output()
        .context("Failed to run git rebase")?;

    if !output.status.success() {
        // Abort rebase
        Command::new("git")
            .args(["rebase", "--abort"])
            .current_dir(wt_path)
            .status()
            .ok();
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Rebase of {source_branch} onto {target_branch} failed: {}",
            stderr.trim()
        );
    }

    // Verify rebase moved source ahead of target
    if !is_ancestor(project_root, target_branch, source_branch)? {
        bail!("Rebase did not advance {source_branch} past {target_branch}");
    }

    // Now ff the target branch from project_root
    merge_ff_only(project_root, target_branch, source_branch)
}

pub fn remote_url(project_root: &str) -> Option<String> {
    Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(project_root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

pub fn push_branch(wt_path: &str, branch: &str) -> Result<()> {
    retry("git", || {
        let status = Command::new("git")
            .args(["push", "origin", branch, "--set-upstream"])
            .current_dir(wt_path)
            .status()
            .context("Failed to push branch")?;

        if !status.success() {
            bail!("Failed to push branch {branch}");
        }
        Ok(())
    })
}

pub fn remote_reachable(wt_path: &str) -> bool {
    Command::new("git")
        .args(["ls-remote", "--exit-code", "origin"])
        .current_dir(wt_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
