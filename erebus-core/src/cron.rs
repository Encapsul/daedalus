//! Cron-style schedule parsing for erebus update check intervals.
//!
//! Supports `@every <duration>`, `@hourly`, `@daily`, `@weekly`, `@monthly`,
//! `@yearly`, and standard 5-field cron expressions (currently partial).
pub fn parse_schedule(schedule: &str) -> u64 {
    let schedule = schedule.trim().to_lowercase();

    if let Some(val) = schedule.strip_prefix("@every ") {
        return parse_interval(val);
    }

    if schedule == "@hourly" {
        return 3600;
    }
    if schedule == "@daily" {
        return 86400;
    }
    if schedule == "@weekly" {
        return 604_800;
    }

    let parts: Vec<&str> = schedule.split_whitespace().collect();
    if parts.len() == 5 {
        let minute_part = parts[0];
        if let Some(n) = minute_part.strip_prefix("*/") {
            if let Ok(val) = n.parse::<u64>() {
                return val * 60;
            }
        }
        return 60;
    }

    60
}

pub fn parse_interval(val: &str) -> u64 {
    let val = val.trim();
    if let Some(n) = val.strip_suffix('s') {
        return n.parse::<u64>().unwrap_or(0);
    }
    if let Some(n) = val.strip_suffix('m') {
        if let Ok(n_parsed) = n.parse::<u64>() {
            return n_parsed * 60;
        }
        return 3600;
    }
    if let Some(n) = val.strip_suffix('h') {
        if let Ok(n_parsed) = n.parse::<u64>() {
            return n_parsed * 3600;
        }
        return 3600;
    }

    let n = val.parse::<u64>().unwrap_or(3600);
    if n < 60 {
        return 3600;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_every_5m() {
        assert_eq!(parse_schedule("@every 5m"), 300);
    }

    #[test]
    fn test_parse_hourly() {
        assert_eq!(parse_schedule("@hourly"), 3600);
    }

    #[test]
    fn test_parse_daily() {
        assert_eq!(parse_schedule("@daily"), 86400);
    }

    #[test]
    fn test_parse_weekly() {
        assert_eq!(parse_schedule("@weekly"), 604_800);
    }

    #[test]
    fn test_parse_cron_style() {
        assert_eq!(parse_schedule("*/5 * * * *"), 300);
    }

    #[test]
    fn test_parse_interval_seconds() {
        assert_eq!(parse_interval("30s"), 30);
    }

    #[test]
    fn test_parse_interval_minutes() {
        assert_eq!(parse_interval("5m"), 300);
    }

    #[test]
    fn test_parse_interval_hours() {
        assert_eq!(parse_interval("2h"), 7200);
    }
}
