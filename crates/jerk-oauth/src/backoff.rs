pub const BACKOFF_DURATION_SECS: u64 = 300;

pub fn should_backoff(last_failure_epoch_secs: Option<u64>, now_epoch_secs: u64) -> bool {
    match last_failure_epoch_secs {
        Some(t) => now_epoch_secs < t.saturating_add(BACKOFF_DURATION_SECS),
        None => false,
    }
}

pub fn backoff_remaining_secs(last_failure_epoch_secs: u64, now_epoch_secs: u64) -> u64 {
    let deadline = last_failure_epoch_secs.saturating_add(BACKOFF_DURATION_SECS);
    deadline.saturating_sub(now_epoch_secs)
}
