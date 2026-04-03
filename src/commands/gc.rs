use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Result;
use tracing::info;

use crate::config::ProjectContext;
use crate::git;
use crate::lock::is_process_alive;
use crate::td;

const TERMINAL_STATUSES: &[&str] = &["done", "approved", "blocked", "closed"];

pub fn run(ctx: &ProjectContext) -> Result<()> {
    let worktrees_removed = gc_worktrees(ctx)?;
    let locks_removed = gc_stale_locks(&ctx.cfg.lock_dir)?;
    println!("gc: {worktrees_removed} worktree(s) removed, {locks_removed} stale lock(s) cleaned");
    Ok(())
}

fn gc_worktrees(ctx: &ProjectContext) -> Result<usize> {
    let entries = git::list_nocturnal_worktrees(&ctx.project_root)?;
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

        info!(
            "gc: removing worktree for {task_id} (status: {status}): {}",
            wt_path.display()
        );

        let branch = format!("nocturnal/{task_id}");
        let rm_status = Command::new("git")
            .args(["gtr", "rm", &branch, "--delete-branch", "--force", "--yes"])
            .current_dir(&ctx.project_root)
            .status();

        match rm_status {
            Ok(s) if s.success() => {
                println!("  removed worktree: {} ({task_id})", wt_path.display());
                removed += 1;
            }
            Ok(s) => {
                info!(
                    "gc: git gtr rm failed for {} (exit: {s})",
                    wt_path.display()
                );
            }
            Err(e) => {
                info!("gc: git gtr rm error for {}: {e:#}", wt_path.display());
            }
        }
    }

    Ok(removed)
}

fn gc_stale_locks(lock_dir: &Path) -> Result<usize> {
    if !lock_dir.is_dir() {
        return Ok(0);
    }

    let mut removed = 0;

    let entries = fs::read_dir(lock_dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.starts_with("nocturnal.")
            || !Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("lock"))
        {
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
