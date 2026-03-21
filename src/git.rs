use std::process::Command;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};

fn retry<F, T>(f: F) -> Result<T>
where
    F: Fn() -> Result<T>,
{
    match f() {
        Ok(val) => Ok(val),
        Err(err) => {
            tracing::warn!("git command failed, retrying in 3s: {err}");
            thread::sleep(Duration::from_secs(3));
            f()
        }
    }
}

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

pub fn worktree_path(project_root: &str, task_id: &str) -> Result<Option<String>> {
    let branch = worktree_branch(task_id);
    let target_ref = format!("refs/heads/{branch}");

    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(project_root)
        .output()
        .context("Failed to list worktrees")?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut current_path: Option<&str> = None;
    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path);
        } else if let Some(branch_ref) = line.strip_prefix("branch ")
            && branch_ref == target_ref
        {
            return Ok(current_path.map(|s| s.to_string()));
        }
    }

    Ok(None)
}

pub fn ensure_worktree(project_root: &str, task_id: &str) -> Result<String> {
    if let Some(path) = worktree_path(project_root, task_id)? {
        return Ok(path);
    }

    let branch = worktree_branch(task_id);
    tracing::info!("Creating worktree: {branch}");

    let status = Command::new("git")
        .args(["gtr", "new", &branch])
        .current_dir(project_root)
        .status()
        .context("Failed to run git gtr new")?;

    if !status.success() {
        bail!("git gtr new {branch} failed");
    }

    worktree_path(project_root, task_id)?
        .ok_or_else(|| anyhow::anyhow!("Worktree not found after creation"))
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
    retry(|| {
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
