//! Only the fields used by the overview account summary cross the IPC boundary.
use crate::http_quota::Failure;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountProfile {
    pub display_name: Option<String>,
    pub username: Option<String>,
    pub lifetime_tokens: Option<u64>,
    pub peak_daily_tokens: Option<u64>,
    pub longest_running_turn_sec: Option<u64>,
    pub longest_streak_days: Option<u64>,
    pub current_streak_days: Option<u64>,
    pub stats_as_of: Option<String>,
    pub stats_unavailable: bool,
}
fn name(v: &Value) -> Option<String> {
    v.as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
        .map(|s| s.trim().into())
}
fn date(v: &Value) -> Option<String> {
    let s = v.as_str()?;
    if s.len() != 10 || chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_err() {
        return None;
    }
    Some(s.into())
}
fn count(v: &Value) -> Option<u64> {
    // Frontend numbers must remain exact integers.
    v.as_u64().filter(|n| *n <= 9_007_199_254_740_991)
}
pub fn normalize(v: &Value) -> Result<AccountProfile, Failure> {
    if !v["profile"].is_object() && !v["stats"].is_object() {
        return Err("unsupported_response".into());
    }
    let s = &v["stats"];
    let unavailable = !s.is_object() || !v["metadata"]["stats_error"].is_null();
    Ok(AccountProfile {
        display_name: name(&v["profile"]["display_name"]),
        username: name(&v["profile"]["username"]),
        stats_as_of: date(&v["metadata"]["stats_as_of"]),
        stats_unavailable: unavailable,
        ..if unavailable {
            AccountProfile::default()
        } else {
            AccountProfile {
                lifetime_tokens: count(&s["lifetime_tokens"]),
                peak_daily_tokens: count(&s["peak_daily_tokens"]),
                longest_running_turn_sec: count(&s["longest_running_turn_sec"]),
                longest_streak_days: count(&s["longest_streak_days"]),
                current_streak_days: count(&s["current_streak_days"]),
                ..Default::default()
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn preserves_unknowns_and_excludes_unused_activity_data() {
        let p = normalize(&json!({"profile":{"display_name":"Test", "username":"user"},"stats":{
            "lifetime_tokens":null,"peak_daily_tokens":0,"current_streak_days":-1,
            "daily_usage_buckets":[{"start_date":"2026-10-06","tokens":2},{"start_date":"2026-10-05","tokens":1}],
            "weekly_usage_buckets":[{"start_date":"2026-02-30","tokens":1}],
            "cumulative_daily_usage_buckets":[{"start_date":"2026-10-06","tokens":1},{"start_date":"2026-10-06","tokens":2}]
        }})).unwrap();
        assert_eq!(p.lifetime_tokens, None);
        assert_eq!(p.peak_daily_tokens, Some(0));
        assert_eq!(p.current_streak_days, None);
        let serialized = serde_json::to_string(&p).unwrap();
        assert!(!serialized.contains("usage_buckets"));
        assert!(!serialized.contains("2026-10-05"));
    }
    #[test]
    fn stats_error_does_not_expose_partial_totals_or_server_message() {
        let p = normalize(&json!({"profile":{"username":"safe"},"stats":{"lifetime_tokens":100},"metadata":{"stats_error":"internal message"}})).unwrap();
        assert!(p.stats_unavailable && p.lifetime_tokens.is_none());
        assert_eq!(p.username.as_deref(), Some("safe"));
        assert!(
            !serde_json::to_string(&p)
                .unwrap()
                .contains("internal message")
        );
        assert!(normalize(&json!({"email":"private"})).is_err());
    }
}
