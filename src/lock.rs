use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use tracing::{info, warn};

pub struct Lock {
    path: PathBuf,
}

impl Lock {
    pub fn acquire(lock_dir: &Path, name: &str) -> Result<Self> {
        let path = lock_dir.join(format!("nocturnal.{name}.lock"));

        if fs::create_dir(&path).is_err() {
            // Check if holding process is still alive
            let pidfile = path.join("pid");
            if let Ok(pid_str) = fs::read_to_string(&pidfile)
                && let Ok(pid) = pid_str.trim().parse::<u32>()
                && is_process_alive(pid)
            {
                bail!("Another '{name}' process is running (PID {pid})");
            }
            // Stale lock — reclaim
            info!("Removing stale lock for '{name}'");
            fs::remove_dir_all(&path).ok();
            if fs::create_dir(&path).is_err() {
                bail!("Failed to acquire lock for '{name}'");
            }
        }

        let pidfile = path.join("pid");
        if let Err(e) = fs::write(&pidfile, std::process::id().to_string()) {
            warn!("failed to write PID file: {e}");
        }

        Ok(Lock { path })
    }

    pub fn try_acquire(lock_dir: &Path, name: &str) -> Option<Self> {
        Self::acquire(lock_dir, name).ok()
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path).ok();
    }
}

pub(crate) fn is_process_alive(pid: u32) -> bool {
    let Ok(pid_i32) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: kill(pid, 0) with signal 0 performs an existence check without
    // sending a signal. The pid comes from our own lock PID files.
    let ret = unsafe { libc::kill(pid_i32, 0) };
    ret == 0 || (ret == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_is_alive() {
        assert!(is_process_alive(std::process::id()));
    }

    #[test]
    fn pid_1_does_not_panic() {
        // PID 1 (init/launchd) exists but may be owned by root.
        // We only verify the call doesn't panic; the result depends on the user.
        let _ = is_process_alive(1);
    }

    #[test]
    fn large_unused_pid_is_dead() {
        // PID 4_000_000 is beyond typical OS limits and should not exist.
        assert!(!is_process_alive(4_000_000));
    }
}
