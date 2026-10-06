use chrono::NaiveDate;
use chrono_tz::{Tz, UTC};
use pulse_core::{
    domain::{SessionMeta, TokenCounts, UsageFact},
    ledger::ParserState,
    storage::*,
};
use serde_json::json;

fn day(value: &str) -> NaiveDate {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
}
fn fact(id: &str, at: &str) -> UsageFact {
    UsageFact {
        id: id.into(),
        session_id: id.into(),
        turn_id: None,
        model: "test-model".into(),
        provider: "test".into(),
        timestamp: at.into(),
        tokens: TokenCounts {
            input: Some(10),
            cached: Some(3),
            output: Some(2),
            total: Some(12),
            ..Default::default()
        },
        service_tier: None,
        request_input: None,
        quality: "test".into(),
        price_version: Some("historical-price".into()),
        cost_nanousd: Some(17),
    }
}
fn commit(store: &mut Store, facts: &[UsageFact], offset: u64, timezone: Tz) {
    let cursor = FileCursor {
        source_id: "test".into(),
        file_key: "test".into(),
        offset,
        file_identity: "test".into(),
        modified_ns: "1".into(),
        parser_version: pulse_core::PARSER_VERSION,
        state: ParserState::default(),
    };
    let sessions = facts
        .iter()
        .map(|f| SessionMeta {
            id: f.session_id.clone(),
            last_activity: f.timestamp.clone(),
            status: "completed".into(),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    store
        .commit_batch(&cursor, &sessions, facts, &[], timezone)
        .unwrap();
}
fn finish(store: &mut Store) {
    while !store.step_timezone_rebuild(2).unwrap().ready {}
}
fn summary(store: &Store, today: &str, tz: Tz) -> serde_json::Value {
    serde_json::to_value(store.summary(day(today), tz).unwrap()).unwrap()
}

#[test]
fn new_calendar_matches_fresh_ledger_and_catches_live_appends_without_touching_facts() {
    let mut facts = vec![
        fact("a", "2026-10-05T23:50:00Z"),
        fact("b", "2026-10-06T00:10:00Z"),
        fact("c", "2026-10-06T17:10:00Z"),
    ];
    facts[1].tokens = TokenCounts {
        total: Some(9),
        ..Default::default()
    };
    facts[1].cost_nanousd = None;
    facts[2].tokens = TokenCounts::default();
    facts[2].cost_nanousd = None;
    let mut store = Store::in_memory().unwrap();
    commit(&mut store, &facts, 3, UTC);
    let old = summary(&store, "2026-10-06", UTC);
    let before_detail = store
        .session_detail(&SessionDetailRequest {
            id: "a".into(),
            models_after: None,
            prices_after: None,
        })
        .unwrap()
        .unwrap();
    store
        .begin_timezone_rebuild(chrono_tz::Asia::Shanghai)
        .unwrap();
    assert_eq!(store.step_timezone_rebuild(1).unwrap().processed, 1);
    assert_eq!(summary(&store, "2026-10-06", UTC), old);
    assert!(matches!(
        store.commit_timezone_rebuild(&[]),
        Err(StoreError::RebuildPending)
    ));
    finish(&mut store);
    facts.push(fact("late", "2026-10-06T16:10:00Z"));
    commit(&mut store, &facts[3..], 4, UTC);
    assert!(matches!(
        store.commit_timezone_rebuild(&[]),
        Err(StoreError::RebuildPending)
    ));
    finish(&mut store);
    assert_eq!(store.timezone_rebuild_status().unwrap().processed, 4);
    store
        .commit_timezone_rebuild(&[(
            "app",
            json!({"timezone":"Asia/Shanghai","other":"retained"}),
        )])
        .unwrap();
    let mut fresh = Store::in_memory().unwrap();
    commit(&mut fresh, &facts, 4, chrono_tz::Asia::Shanghai);
    assert_eq!(
        summary(&store, "2026-10-07", chrono_tz::Asia::Shanghai),
        summary(&fresh, "2026-10-07", chrono_tz::Asia::Shanghai)
    );
    assert_eq!(store.fact_count().unwrap(), 4);
    assert_eq!(store.cursor("test", "test").unwrap().unwrap().offset, 4);
    assert_eq!(
        serde_json::to_value(
            store
                .session_detail(&SessionDetailRequest {
                    id: "a".into(),
                    models_after: None,
                    prices_after: None,
                })
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(before_detail).unwrap()
    );
    store.begin_timezone_rebuild(UTC).unwrap();
    finish(&mut store);
    store.commit_timezone_rebuild(&[]).unwrap();
    let mut fresh = Store::in_memory().unwrap();
    commit(&mut fresh, &facts, 4, UTC);
    assert_eq!(
        summary(&store, "2026-10-07", UTC),
        summary(&fresh, "2026-10-07", UTC)
    );
}

#[test]
fn cancellation_and_restart_keep_the_last_published_timezone_and_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pulse.sqlite");
    let mut store = Store::open(&path).unwrap();
    commit(&mut store, &[fact("a", "2026-10-05T23:50:00Z")], 1, UTC);
    store
        .set_setting("app", &json!({"timezone":"UTC"}))
        .unwrap();
    let old = summary(&store, "2026-10-06", UTC);
    store
        .begin_timezone_rebuild(chrono_tz::Asia::Shanghai)
        .unwrap();
    store.step_timezone_rebuild(1).unwrap();
    store.cancel_timezone_rebuild().unwrap();
    assert!(store.timezone_rebuild_status().is_none());
    assert_eq!(summary(&store, "2026-10-06", UTC), old);
    store
        .begin_timezone_rebuild(chrono_tz::Asia::Shanghai)
        .unwrap();
    finish(&mut store);
    drop(store);
    let store = Store::open(&path).unwrap();
    assert!(store.timezone_rebuild_status().is_none());
    assert_eq!(summary(&store, "2026-10-06", UTC), old);
    assert_eq!(
        store.setting::<serde_json::Value>("app").unwrap().unwrap(),
        json!({"timezone":"UTC"})
    );
    assert_eq!(store.cursor("test", "test").unwrap().unwrap().offset, 1);
}

#[test]
fn publishing_buckets_and_settings_is_atomic_and_failed_publication_can_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pulse.sqlite");
    let mut store = Store::open(&path).unwrap();
    commit(&mut store, &[fact("a", "2026-10-05T23:50:00Z")], 1, UTC);
    store
        .set_setting("app", &json!({"timezone":"UTC"}))
        .unwrap();
    let old = summary(&store, "2026-10-06", UTC);
    store
        .begin_timezone_rebuild(chrono_tz::Asia::Shanghai)
        .unwrap();
    finish(&mut store);
    let other = rusqlite::Connection::open(&path).unwrap();
    other.execute_batch("CREATE TRIGGER fail_setting BEFORE INSERT ON settings WHEN NEW.key='app' BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert!(
        store
            .commit_timezone_rebuild(&[("app", json!({"timezone":"Asia/Shanghai"}))])
            .is_err()
    );
    assert_eq!(summary(&store, "2026-10-06", UTC), old);
    assert_eq!(
        store.setting::<serde_json::Value>("app").unwrap().unwrap(),
        json!({"timezone":"UTC"})
    );
    assert!(store.timezone_rebuild_status().unwrap().ready);
    other.execute_batch("DROP TRIGGER fail_setting").unwrap();
    store
        .commit_timezone_rebuild(&[("app", json!({"timezone":"Asia/Shanghai"}))])
        .unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.setting::<serde_json::Value>("app").unwrap().unwrap(),
        json!({"timezone":"Asia/Shanghai"})
    );
    assert_eq!(
        store
            .summary(day("2026-10-06"), chrono_tz::Asia::Shanghai)
            .unwrap()
            .days
            .last()
            .unwrap()
            .total,
        12
    );
}

#[test]
fn merged_calendar_overflow_aborts_a_whole_batch_without_advancing_its_cursor() {
    let mut store = Store::in_memory().unwrap();
    let mut a = fact("a", "2026-10-05T23:00:00Z");
    a.tokens = TokenCounts {
        total: Some(i64::MAX as u64),
        ..Default::default()
    };
    a.cost_nanousd = None;
    let mut b = fact("b", "2026-10-06T01:00:00Z");
    b.tokens = TokenCounts {
        total: Some(1),
        ..Default::default()
    };
    b.cost_nanousd = None;
    commit(&mut store, &[a, b], 2, UTC);
    store
        .begin_timezone_rebuild(chrono_tz::Asia::Shanghai)
        .unwrap();
    assert!(store.step_timezone_rebuild(2).is_err());
    assert_eq!(store.timezone_rebuild_status().unwrap().processed, 0);
    assert_eq!(
        store.model_usage("2026-10-05", "2026-10-05", UTC).unwrap()[0].total,
        i64::MAX as u64
    );
    assert_eq!(
        store.model_usage("2026-10-06", "2026-10-06", UTC).unwrap()[0].total,
        1
    );
    store.cancel_timezone_rebuild().unwrap();
    assert_eq!(store.fact_count().unwrap(), 2);
}

#[test]
fn calendar_boundaries_cover_dst_midnight_gaps_and_skipped_dates() {
    for (zone, date, at) in [
        ("America/New_York", "2026-03-08", "2026-03-08T07:00:00Z"),
        ("America/New_York", "2026-11-01", "2026-11-01T05:30:00Z"),
        ("America/Sao_Paulo", "2018-11-04", "2018-11-04T03:00:00Z"),
        ("Pacific/Apia", "2011-12-31", "2011-12-30T10:00:00Z"),
    ] {
        let zone = zone.parse::<Tz>().unwrap();
        let mut store = Store::in_memory().unwrap();
        commit(&mut store, &[fact("a", at)], 1, UTC);
        store.begin_timezone_rebuild(zone).unwrap();
        finish(&mut store);
        store.commit_timezone_rebuild(&[]).unwrap();
        let result = store.summary(day(date), zone).unwrap();
        assert_eq!(result.days.last().unwrap().total, 12);
        assert_eq!(result.sessions, 1);
        let request = SessionPageRequest {
            filter: SessionFilter {
                from_day: Some(date.into()),
                through_day: Some(date.into()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(store.session_page(&request, zone).unwrap().items.len(), 1);
        if zone == chrono_tz::Pacific::Apia {
            assert_eq!(
                store
                    .independent_sessions("2011-12-30", "2011-12-30", zone)
                    .unwrap(),
                0
            );
        }
    }
}

#[test]
fn rebuild_batch_is_bounded_and_preserves_active_queries_between_steps() {
    let mut store = Store::in_memory().unwrap();
    let facts = (0..1000)
        .map(|i| fact(&format!("f{i}"), "2026-10-06T00:10:00Z"))
        .collect::<Vec<_>>();
    commit(&mut store, &facts, 1000, UTC);
    store
        .begin_timezone_rebuild(chrono_tz::Asia::Shanghai)
        .unwrap();
    assert!(matches!(
        store.step_timezone_rebuild(0),
        Err(StoreError::Query)
    ));
    assert!(matches!(
        store.step_timezone_rebuild(513),
        Err(StoreError::Query)
    ));
    assert_eq!(store.step_timezone_rebuild(256).unwrap().processed, 256);
    assert_eq!(store.summary(day("2026-10-06"), UTC).unwrap().total, 12_000);
    assert_eq!(
        store
            .session_page(&Default::default(), UTC)
            .unwrap()
            .items
            .len(),
        10
    );
    finish(&mut store);
    store.commit_timezone_rebuild(&[]).unwrap();
    assert_eq!(
        store
            .summary(day("2026-10-06"), chrono_tz::Asia::Shanghai)
            .unwrap()
            .total,
        12_000
    );
}
