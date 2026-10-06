use pulse_core::quota::{merged, normalize};
use serde_json::json;
#[test]
fn primary_label_and_remaining_come_from_real_bucket_fields() {
    let rows = normalize(
        "windows",
        &json!({}),
        &json!({"rateLimitsByLimitId":{"codex":{"limitId":"codex","primary":{"usedPercent":57,"windowDurationMins":10080,"resetsAt":1791601434},"secondary":null}}}),
        "2026-10-06T00:00:00Z",
    );
    assert_eq!(
        rows[0].primary.as_ref().unwrap().remaining_percent,
        Some(43.0)
    );
    assert_eq!(
        rows[0].primary.as_ref().unwrap().duration_minutes,
        Some(10080)
    );
    assert!(rows[0].secondary.is_none());
}
#[test]
fn unknown_identity_stays_separate_and_null_is_not_zero() {
    let response = json!({"rateLimits":{"primary":{"usedPercent":null}}});
    let mut rows = normalize("windows", &json!({}), &response, "2026-10-06T00:00:00Z");
    rows.extend(normalize(
        "wsl:Ubuntu",
        &json!({}),
        &response,
        "2026-10-06T00:00:01Z",
    ));
    let rows = merged(rows);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].primary.as_ref().unwrap().remaining_percent, None);
    assert!(!rows[0].identity_confirmed);
}
#[test]
fn same_confirmed_scope_uses_latest_without_summing() {
    let account = json!({"workspaceRouting":{"chatgptAccountId":"workspace-test","backendOrigin":"https://example.invalid"}});
    let response = json!({"accountId":"account-test","rateLimits":{"planType":"pro","primary":{"usedPercent":25}}});
    let mut rows = normalize("windows", &account, &response, "2026-10-06T00:00:00Z");
    rows.extend(normalize(
        "wsl:Ubuntu",
        &account,
        &response,
        "2026-10-06T00:00:01Z",
    ));
    let rows = merged(rows);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].primary.as_ref().unwrap().remaining_percent,
        Some(75.0)
    );
}
