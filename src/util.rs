use std::thread;
use std::time::Duration;

use anyhow::Result;

pub(crate) fn retry<F, T>(context: &str, f: F) -> Result<T>
where
    F: Fn() -> Result<T>,
{
    match f() {
        Ok(val) => Ok(val),
        Err(err) => {
            tracing::warn!("{context} command failed, retrying in 3s: {err}");
            thread::sleep(Duration::from_secs(3));
            f()
        }
    }
}
