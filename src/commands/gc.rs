use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Result;
use tracing::info;

use crate::config::ProjectContext;
use crate::lock::is_process_alive;
use crate::td;

const TERMINAL_STATUSES: &[&str] = &["done", "approved", "blocked", "closed"];

pub fn run(ctx: &ProjectContext) -> Result<()> {
    let worktrees_removed = gc_worktrees(ctx)?;
    let locks_removed = gc_stale_locks(&ctx.cfg.lock_dir)?;
    println!(
        "gc: {} worktree(s) removed, {} stale lock(s) cleaned",
        worktrees_removed, locks_removed
    );
    Ok(())
}

fn gc_worktrees(ctx: &ProjectContext) -> Result<usize> {
    let entries = list_nocturnal_worktrees(&ctx.project_root)?;
    let td = td::Td::new(&ctx.project_root);
    let mut removed = 0;

    for (wt_path, task_id) in entries {
        let status = match td.show(&task_id) {
            Ok(task) => task.status,
            Err(e) => {
                info!("gc: could not query task {task_id}: {e:#}, skipping");
                continue;
            }
        };

        if !TERMINAL_STATUSES.contains(&status.as_str()) {
            info!("gc: keeping worktree for {task_id} (status: {status})");
            continue;
        }

        info!("gc: removing worktree for {task_id} (status: {status}): {wt_path}");

        let rm_status = Command::new("git")
            .args(["worktree", "remove", "--force", &wt_path])
            .current_dir(&ctx.project_root)
            .status();

        match rm_status {
            Ok(s) if s.success() => {
                println!("  removed worktree: {wt_path} ({task_id})");
                removed += 1;
            }
            Ok(s) => {
                info!("gc: git worktree remove failed for {wt_path} (exit: {s})");
            }
            Err(e) => {
                info!("gc: git worktree remove error for {wt_path}: {e:#}");
            }
        }

        // Delete the local branch
        let branch = format!("nocturnal/{task_id}");
        let del_status = Command::new("git")
            .args(["branch", "-D", &branch])
            .current_dir(&ctx.project_root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        if del_status.is_ok_and(|s| s.success()) {
            info!("gc: deleted branch {branch}");
        }
    }

    Ok(removed)
}

/// Parse `git worktree list --porcelain` and return (path, task_id) pairs for
/// worktrees on branches matching `refs/heads/nocturnal/<task_id>`.
fn list_nocturnal_worktrees(project_root: &str) -> Result<Vec<(String, String)>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(project_root)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut result = Vec::new();
    let mut current_path: Option<String> = None;

    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path.to_string());
        } else if let Some(branch_ref) = line.strip_prefix("branch ") {
            if let Some(task_id) = branch_ref.strip_prefix("refs/heads/nocturnal/") {
                if let Some(path) = current_path.take() {
                    result.push((path, task_id.to_string()));
                }
            }
        }
    }

    Ok(result)
}

fn gc_stale_locks(lock_dir: &str) -> Result<usize> {
    let dir = PathBuf::from(lock_dir);
    if !dir.is_dir() {
        return Ok(0);
    }

    let mut removed = 0;

    let entries = fs::read_dir(&dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.starts_with("nocturnal.") || !name.ends_with(".lock") {
            continue;
        }

        let pidfile = path.join("pid");
        let is_alive = fs::read_to_string(&pidfile)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .is_some_and(is_process_alive);

        if is_alive {
            continue;
        }

        info!("gc: removing stale lock: {}", path.display());
        match fs::remove_dir_all(&path) {
            Ok(()) => {
                println!("  removed stale lock: {name}");
                removed += 1;
            }
            Err(e) => {
                info!("gc: failed to remove stale lock {}: {e:#}", path.display());
            }
        }
    }

    Ok(removed)
}
