pub fn compute_expiry(
    now_epoch_secs: u64,
    expires_in: Option<u64>,
    max_age_ms: Option<u64>,
) -> Option<u64> {
    match (expires_in, max_age_ms) {
        (Some(ei), Some(ma)) => {
            let from_ei = now_epoch_secs.checked_add(ei)?;
            let from_ma = now_epoch_secs.checked_add(ma / 1000)?;
            Some(from_ei.min(from_ma))
        }
        (Some(ei), None) => now_epoch_secs.checked_add(ei),
        (None, Some(ma)) => now_epoch_secs.checked_add(ma / 1000),
        (None, None) => None,
    }
}

pub fn is_expired(expires_at: u64, now_epoch_secs: u64) -> bool {
    now_epoch_secs >= expires_at
}

pub fn needs_refresh(expires_at: u64, now_epoch_secs: u64, margin_secs: u64) -> bool {
    now_epoch_secs + margin_secs >= expires_at
}

pub const REFRESH_MARGIN_SECS: u64 = 300;
