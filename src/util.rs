use std::time::SystemTime;

use crate::discord::{Id, DISCORD_EPOCH};

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 60 * SECONDS_PER_MINUTE;
const SECONDS_PER_DAY: u64 = 24 * SECONDS_PER_HOUR;

pub fn max_snowflake_now() -> u64 {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    (now - DISCORD_EPOCH) << 22
}

// expects duration in seconds
pub fn format_duration(duration: u64) -> String {
    let mut duration = duration;
    let mut result = String::new();

    let days = duration / SECONDS_PER_DAY;
    if days > 0 {
        result.push_str(&format!("{}d", days));
        duration %= SECONDS_PER_DAY;
    }

    let hours = duration / SECONDS_PER_HOUR;
    if hours > 0 {
        result.push_str(&format!("{}h", hours));
        duration %= SECONDS_PER_HOUR;
    }

    let minutes = duration / SECONDS_PER_MINUTE;
    if minutes > 0 {
        result.push_str(&format!("{}m", minutes));
        duration %= SECONDS_PER_MINUTE;
    }

    if duration > 0 {
        result.push_str(&format!("{}s", duration));
    }

    if result.is_empty() {
        result.push_str("0s");
    }

    result
}

pub fn duration_to_unix_timestamp(duration: u64) -> u64 {
    let now = chrono::Utc::now().timestamp() as u64;

    now + duration
}

pub fn snowflake_to_timestamp(snowflake: Id) -> u64 {
    (snowflake.get() >> 22) + DISCORD_EPOCH
}
