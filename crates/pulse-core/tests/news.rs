use pulse_core::news::{NewsKind, challenge_page, history_items, merge, status_items};
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

#[test]
fn challenge_keeps_improvements_separate_from_resets_and_ignores_votes_scripts() {
    let page = r#"<aside data-challenge-clock data-start="2026-10-05" data-days="28"></aside>
    <ol class="challenge-ledger">
      <li class="challenge-row" id="day-2"><div class="challenge-row-date"><time datetime="2026-10-06"></time></div><span>等待 Tibo</span></li>
      <li class="challenge-row" id="day-1"><div class="challenge-row-date"><time datetime="2026-10-05"></time></div>
        <article class="challenge-entry challenge-entry--improvement"><h3><a href="https://x.com/thsottiaux/status/example">速度 &amp; 质量</a></h3><div class="challenge-votes">123456</div><p>产品改进</p><script>private_script</script></article>
        <article class="challenge-entry challenge-entry--reset"><h3><a href="javascript:alert(1)">重置</a></h3><p>额度公告</p></article>
      </li>
    </ol>"#;
    let data = challenge_page(page).unwrap();
    assert_eq!(data.timezone, "America/Los_Angeles");
    assert_eq!(data.records[0].day, 2);
    assert!(data.records[0].entries.is_empty());
    assert_eq!(data.records[1].entries[0].title, "速度 & 质量");
    assert_eq!(data.records[1].entries[0].text, "产品改进");
    assert_eq!(data.records[1].entries[0].kind, "improvement");
    assert_eq!(data.records[1].entries[1].kind, "reset");
    assert!(data.records[1].entries[1].source_url.is_none());
    let stored = serde_json::to_string(&data).unwrap();
    assert!(!stored.contains("123456") && !stored.contains("private_script"));
}

#[test]
fn challenge_markup_changes_and_invalid_dates_are_errors_not_empty_success() {
    assert!(challenge_page("<html>Unknown future page</html>").is_err());
    let page = r#"<aside data-challenge-clock data-start="2026-10-05" data-days="28"></aside><ol class="challenge-ledger"><li class="challenge-row" id="day-1"><div class="challenge-row-date"><time datetime="2026-10-06"></time></div></li></ol>"#;
    assert!(challenge_page(page).is_err());
    assert!(challenge_page(&"x".repeat(512 * 1024 + 1)).is_err());
}

#[test]
fn cancelled_schedule_is_not_a_confirmed_next_reset() {
    let data =
        json!({"meta":{"api_version":"v1"},"data":{"scheduled_reset":{"status":"cancelled"}}});
    assert!(status_items(&data).unwrap().is_empty());
}
