use pulse_core::news::{NewsKind, history_items, merge, status_items};
use serde_json::json;
#[test]
fn expired_schedule_is_still_a_schedule_not_account_recovery() {
    let data = json!({"meta":{"api_version":"v1"},"data":{"latest_reset":null,"active_watch":null,"scheduled_reset":{"id":"test","status":"scheduled","reset_type":"regular","announced_at":"2026-01-01T00:00:00Z","scheduled_for":"2026-01-02T00:00:00Z","text":"Test scheduled announcement","source":{"type":"x_post","author":"thsottiaux","url":"https://x.com/thsottiaux/status/test"}}}});
    let items = status_items(&data).unwrap();
    assert_eq!(items[0].kind, NewsKind::Scheduled);
    assert!(items[0].probability.is_none());
}
#[test]
fn public_observation_and_forecast_remain_distinct_and_deduplicate() {
    let observed = json!({"id":"observed-test","reset_type":"regular","announced_at":"2026-01-01T00:00:00Z","text":"Synthetic observation","source":{"type":"observed"}});
    let status = json!({"meta":{"api_version":"v1"},"data":{"latest_reset":observed,"scheduled_reset":null,"active_watch":{"level":"strong","reset_chance_percent":null,"forecast_window":"Synthetic window","observed_at":"2026-01-02T00:00:00Z","expires_at":"2026-01-03T00:00:00Z","text":"Synthetic prediction","source":{"type":"observed"}}}});
    let history = json!({"meta":{"api_version":"v1"},"data":[observed]});
    let items = merge(
        status_items(&status).unwrap(),
        history_items(&history).unwrap(),
    );
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].kind, NewsKind::Forecast);
    assert_eq!(items[1].kind, NewsKind::Observation);
    assert!(items[0].probability.is_none());
}
#[test]
fn unknown_api_shape_is_reported_instead_of_empty_success() {
    assert!(status_items(&json!({"data":{}})).is_err());
    assert!(history_items(&json!({"meta":{"api_version":"v1"},"data":{}})).is_err());
}
