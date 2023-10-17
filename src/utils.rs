use std::time::Duration;
use tokio::time::sleep;

pub fn sleep_secs(n: usize) -> tokio::time::Sleep {
    sleep(Duration::from_secs(n as u64))
}
