use chrono::{TimeZone, Utc};

/// Returns a human-readable relative time string given a diff in seconds from now.
/// Negative diffs ("in the future") are also handled.
pub fn time_ago(diff: i64) -> String {
    if diff < 0 {
        String::from("in the future")
    } else if diff < 60 {
        format!("{}s ago", diff)
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 7 * 86400 {
        format!("{}d ago", diff / 86400)
    } else if diff < 30 * 86400 {
        format!("{}w ago", diff / (7 * 86400))
    } else if diff < 365 * 86400 {
        format!("{}mo ago", diff / (30 * 86400))
    } else {
        format!("{}y ago", diff / (365 * 86400))
    }
}

/// Converts a Unix timestamp (seconds) into an `(absolute_str, relative_str)` pair.
/// Returns empty strings if the timestamp is zero.
pub fn format_modified(unix_secs: u64) -> (String, String) {
    if unix_secs == 0 {
        return (String::new(), String::from("unknown"));
    }
    if let Some(dt) = Utc.timestamp_opt(unix_secs as i64, 0).single() {
        let abs = dt.format("%Y-%m-%d %H:%M:%S UTC").to_string();
        let diff = Utc::now().timestamp() - dt.timestamp();
        let rel = time_ago(diff);
        return (abs, rel);
    }
    (String::new(), String::from("unknown"))
}
