use std::thread;
use std::time::Duration;

use anyhow::Result;

pub(crate) fn retry<F, T>(context: &str, f: F) -> Result<T>
where
    F: Fn() -> Result<T>,
{
    let delays = [1u64, 3, 9];
    let mut last_err = None;
    for (attempt, delay) in std::iter::once(&0u64).chain(delays.iter()).enumerate() {
        if attempt > 0 {
            tracing::warn!(
                "{context} attempt {attempt} failed, retrying in {delay}s: {}",
                last_err.as_ref().unwrap()
            );
            thread::sleep(Duration::from_secs(*delay));
        }
        match f() {
            Ok(val) => return Ok(val),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap())
}
