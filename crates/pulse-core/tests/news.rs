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

#[test]
fn public_reset_stats_require_full_valid_history_for_longest_wait() {
    let row = |id: &str, at: &str| json!({"id":id,"reset_type":"regular","announced_at":at,"text":"Synthetic public record","source":{"type":"x_post","author":"thsottiaux"}});
    let status = json!({"data":{"stats":{"total":57,"avg_interval_days":6.8}}});
    let mut history = json!({"meta":{"api_version":"v1"},"pagination":{"has_more":false},"data":[row("b","2026-01-12T00:00:00Z"),row("a","2026-01-01T00:00:00Z")]});
    let result = pulse_core::news::reset_stats(&status, &history);
    assert_eq!(result.total, Some(57));
    assert_eq!(result.average_interval_days, Some(6.8));
    assert_eq!(result.longest_wait_days, Some(11.0));
    assert!(result.history_complete);
    history["pagination"]["has_more"] = json!(true);
    let partial = pulse_core::news::reset_stats(&status, &history);
    assert_eq!(partial.total, Some(57));
    assert!(partial.longest_wait_days.is_none());
    assert!(!partial.history_complete);
    history["pagination"]["has_more"] = json!(false);
    history["data"][0]["announced_at"] = json!("invalid date");
    assert!(!pulse_core::news::reset_stats(&status, &history).history_complete);
    assert_eq!(
        pulse_core::news::reset_stats(&json!(null), &json!(null)),
        pulse_core::news::ResetStats::default()
    );
}

#[test]
fn oversized_history_is_an_error_instead_of_silent_truncation() {
    let rows = (0..=pulse_core::news::MAX_HISTORY_ROWS).map(|id| json!({"id":id.to_string(),"reset_type":"regular","announced_at":"2026-01-01T00:00:00Z","text":"Synthetic reset","source":{"type":"observed"}})).collect::<Vec<_>>();
    let history = json!({"meta":{"api_version":"v1"},"data":rows,"pagination":{"has_more":false}});
    assert_eq!(history_items(&history).unwrap_err(), "history_too_large");
    assert!(!pulse_core::news::reset_stats(&json!(null), &history).history_complete);
}

#[test]
fn active_watch_projection_keeps_signal_separate_from_long_history() {
    let status = json!({"meta":{"api_version":"v1"},"data":{"active_watch":{"level":"elevated","reset_chance_percent":40,"observed_at":"2026-10-06T22:00:00Z","expires_at":"2026-10-07T07:00:00Z","text":"Synthetic watch clue","source":{"type":"x_post","author":"thsottiaux"}}}});
    let watch = pulse_core::news::active_watch(&status).unwrap();
    assert_eq!(watch.level.as_deref(), Some("elevated"));
    assert_eq!(watch.item.kind, NewsKind::Forecast);
    assert_eq!(watch.item.probability, Some(40));
    assert_eq!(watch.item.scheduled_for, None);
    let rows = (0..100).map(|id| json!({"id":id.to_string(),"announced_at":"2026-10-07T00:00:00Z","text":"Synthetic history","source":{"type":"observed"}})).collect::<Vec<_>>();
    let merged = merge(
        status_items(&status).unwrap(),
        history_items(&json!({"meta":{"api_version":"v1"},"data":rows})).unwrap(),
    );
    assert_eq!(merged.len(), 100);
    assert!(!merged.iter().any(|item| item.kind == NewsKind::Forecast));
    assert_eq!(pulse_core::news::active_watch(&status), Some(watch));
    let mut invalid = status.clone();
    invalid["data"]["active_watch"]["expires_at"] = json!("bad");
    assert!(pulse_core::news::active_watch(&invalid).is_none());
    invalid["data"]["active_watch"] = json!(null);
    assert!(pulse_core::news::active_watch(&invalid).is_none());
}
