use std::path::Path;
use std::process::Command;

use anyhow::Result;
use tracing::{debug, error, warn};

use crate::config::ProjectContext;
use crate::project_config::VcsMode;

/// Default disk space threshold in MB.
const DEFAULT_DISK_THRESHOLD_MB: u64 = 500;

/// Errors produced by individual pre-flight checks.
#[derive(Debug)]
pub enum PreflightError {
    DirtyWorkingTree { details: String },
    TdNotFunctional { details: String },
    InsufficientDiskSpace { available_mb: u64, required_mb: u64 },
}

impl std::fmt::Display for PreflightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreflightError::DirtyWorkingTree { details } => {
                write!(
                    f,
                    "Working tree is dirty — commit or stash changes before running nocturnal\n{details}"
                )
            }
            PreflightError::TdNotFunctional { details } => {
                write!(
                    f,
                    "td CLI is not functional — check td installation and project setup\n{details}"
                )
            }
            PreflightError::InsufficientDiskSpace {
                available_mb,
                required_mb,
            } => {
                write!(
                    f,
                    "Insufficient disk space — {available_mb}MB available, {required_mb}MB required"
                )
            }
        }
    }
}

/// Check 1: git working tree clean in project root.
fn check_git_clean(project_root: &Path) -> Result<(), PreflightError> {
    debug!("Pre-flight: checking git working tree is clean");
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(project_root)
        .output()
        .map_err(|e| PreflightError::DirtyWorkingTree {
            details: format!("Failed to run git status: {e}"),
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.trim().is_empty() {
        return Err(PreflightError::DirtyWorkingTree {
            details: stdout.trim().to_string(),
        });
    }
    Ok(())
}

/// Check 2: VCS remote reachable. Warns only — does not fail.
fn check_vcs_remote(project_root: &Path) {
    debug!("Pre-flight: checking VCS remote reachability");
    match Command::new("git")
        .args(["ls-remote", "--exit-code", "origin", "HEAD"])
        .current_dir(project_root)
        .output()
    {
        Err(e) => warn!("Pre-flight: could not run git ls-remote: {e}"),
        Ok(out) if !out.status.success() => {
            warn!("Pre-flight: remote origin is unreachable — check network or VCS config");
        }
        Ok(_) => {}
    }
}

/// Check 3: td CLI is functional.
fn check_td_functional(project_root: &Path) -> Result<(), PreflightError> {
    debug!("Pre-flight: checking td CLI is functional");
    let output = Command::new("td")
        .args(["list", "-w", &project_root.to_string_lossy()])
        .output()
        .map_err(|e| PreflightError::TdNotFunctional {
            details: format!("Failed to run td: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(PreflightError::TdNotFunctional {
            details: stderr.trim().to_string(),
        });
    }
    Ok(())
}

/// Parse the available-space field (KB) from `df -k` output and convert to MB.
/// Returns `None` if the output cannot be parsed.
pub(crate) fn parse_df_available_mb(df_output: &str) -> Option<u64> {
    // `df -k` format: Filesystem  1K-blocks  Used  Available  Use%  Mounted
    // Data is on the second line; Available is at field index 3.
    df_output.lines().nth(1).and_then(|line| {
        line.split_whitespace()
            .nth(3)
            .and_then(|s| s.parse::<u64>().ok())
            .map(|kb| kb / 1024)
    })
}

/// Check 4: sufficient disk space on the filesystem containing `project_root`.
fn check_disk_space(project_root: &Path, threshold_mb: u64) -> Result<(), PreflightError> {
    debug!("Pre-flight: checking disk space (threshold: {threshold_mb}MB)");
    let output = Command::new("df")
        .args(["-k", &project_root.to_string_lossy()])
        .output()
        .map_err(|_| PreflightError::InsufficientDiskSpace {
            available_mb: 0,
            required_mb: threshold_mb,
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    match parse_df_available_mb(&stdout) {
        Some(avail_mb) if avail_mb < threshold_mb => Err(PreflightError::InsufficientDiskSpace {
            available_mb: avail_mb,
            required_mb: threshold_mb,
        }),
        Some(_) => Ok(()),
        None => {
            warn!("Pre-flight: could not parse df output, skipping disk space check");
            Ok(())
        }
    }
}

/// Run all pre-flight checks before spending Claude budget.
///
/// All hard-fail checks run regardless of individual failures so that the
/// operator sees every problem at once. Returns `Err` if any hard check fails.
/// VCS remote reachability is a warn-only check and never causes `Err`.
pub fn run_checks(ctx: &ProjectContext) -> Result<()> {
    let mut failures: Vec<String> = Vec::new();

    // 1. Git working tree clean
    match check_git_clean(&ctx.project_root) {
        Ok(()) => debug!("Pre-flight: git working tree is clean"),
        Err(e) => {
            error!("Pre-flight FAIL: {e}");
            failures.push(e.to_string());
        }
    }

    // 2. VCS remote reachable (warn only)
    match ctx.settings.vcs_mode {
        VcsMode::Auto | VcsMode::GitHub | VcsMode::GitLab => {
            check_vcs_remote(&ctx.project_root);
        }
        VcsMode::Local | VcsMode::Off => {
            debug!(
                "Pre-flight: skipping remote check (vcs.mode={:?})",
                ctx.settings.vcs_mode
            );
        }
    }

    // 3. td functional
    match check_td_functional(&ctx.project_root) {
        Ok(()) => debug!("Pre-flight: td is functional"),
        Err(e) => {
            error!("Pre-flight FAIL: {e}");
            failures.push(e.to_string());
        }
    }

    // 4. Disk space
    match check_disk_space(&ctx.project_root, DEFAULT_DISK_THRESHOLD_MB) {
        Ok(()) => debug!("Pre-flight: disk space OK"),
        Err(e) => {
            error!("Pre-flight FAIL: {e}");
            failures.push(e.to_string());
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("Pre-flight checks failed:\n{}", failures.join("\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_df_linux_format() {
        let output = "Filesystem     1K-blocks      Used Available Use% Mounted on\n\
                      /dev/sda1      102400000  50000000  52400000  49% /\n";
        assert_eq!(parse_df_available_mb(output), Some(51171)); // 52400000 / 1024
    }

    #[test]
    fn parse_df_macos_format() {
        let output = "Filesystem 1024-blocks      Used Available Capacity  iused ifree %iused  Mounted on\n\
                      /dev/disk1   976490576 600000000 376490576    62% 1234567 9876543   11%   /\n";
        assert_eq!(parse_df_available_mb(output), Some(367666)); // 376490576 / 1024
    }

    #[test]
    fn parse_df_sufficient_space() {
        let output = "Filesystem  1K-blocks    Used Available Use% Mounted on\n\
                      /dev/sda1  2000000000 1000000  1999000000  1%  /\n";
        let mb = parse_df_available_mb(output).unwrap();
        assert!(mb > 500, "expected >500MB available, got {mb}");
    }

    #[test]
    fn parse_df_below_threshold() {
        // 100MB available
        let output = "Filesystem  1K-blocks  Used  Available Use% Mounted on\n\
                      /dev/sda1   1024000    922400  102400    91% /\n";
        let mb = parse_df_available_mb(output).unwrap();
        assert!(mb < DEFAULT_DISK_THRESHOLD_MB);
    }

    #[test]
    fn parse_df_empty_output_returns_none() {
        assert_eq!(parse_df_available_mb(""), None);
    }

    #[test]
    fn parse_df_header_only_returns_none() {
        let output = "Filesystem  1K-blocks  Used  Available Use% Mounted on\n";
        assert_eq!(parse_df_available_mb(output), None);
    }

    #[test]
    fn parse_df_malformed_available_returns_none() {
        let output = "Filesystem  1K-blocks  Used  -  Use% Mounted on\n\
                      /dev/sda1   1024000    922400  -  91% /\n";
        assert_eq!(parse_df_available_mb(output), None);
    }

    #[test]
    fn preflight_error_display_dirty_tree() {
        let e = PreflightError::DirtyWorkingTree {
            details: "M  foo.rs".to_string(),
        };
        let msg = e.to_string();
        assert!(msg.contains("dirty"));
        assert!(msg.contains("foo.rs"));
    }

    #[test]
    fn preflight_error_display_td_not_functional() {
        let e = PreflightError::TdNotFunctional {
            details: "command not found".to_string(),
        };
        let msg = e.to_string();
        assert!(msg.contains("td CLI"));
        assert!(msg.contains("command not found"));
    }

    #[test]
    fn preflight_error_display_insufficient_disk() {
        let e = PreflightError::InsufficientDiskSpace {
            available_mb: 100,
            required_mb: 500,
        };
        let msg = e.to_string();
        assert!(msg.contains("100MB"));
        assert!(msg.contains("500MB"));
    }
}
