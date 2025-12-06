pub const CACHE_TTL_HOURS: u64 = 168; // 1 week
pub const REFRESH_THRESHOLD_PERCENT: u64 = 10; // Trigger refresh after 10% of TTL (~17 hours)
pub const MIN_REFRESH_INTERVAL_SECS: i64 = 3600; // 1 hour cooldown between refresh attempts
pub const LOCK_TIMEOUT_SECS: i64 = 300; // 5 minutes
pub const STALE_LOCK_THRESHOLD_SECS: i64 = 600; // 10 minutes
