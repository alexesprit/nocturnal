use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Result, bail};
use tracing::info;

pub struct Lock {
    path: PathBuf,
}

impl Lock {
    pub fn acquire(lock_dir: &str, name: &str) -> Result<Self> {
        let path = PathBuf::from(lock_dir).join(format!("nocturnal.{name}.lock"));

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
        fs::write(&pidfile, std::process::id().to_string()).ok();

        Ok(Lock { path })
    }

    pub fn try_acquire(lock_dir: &str, name: &str) -> Option<Self> {
        Self::acquire(lock_dir, name).ok()
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path).ok();
    }
}

fn is_process_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
