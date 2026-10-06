use chrono_tz::{Tz, UTC};
use pulse_core::{
    domain::{SessionMeta, TokenCounts, UsageFact},
    ledger::ParserState,
    storage::*,
};

fn meta(id: &str, at: &str) -> SessionMeta {
    SessionMeta {
        id: id.into(),
        last_activity: at.into(),
        status: "completed".into(),
        ..Default::default()
    }
}
fn fact(id: &str, session: &str, model: &str, at: &str) -> UsageFact {
    UsageFact {
        id: id.into(),
        session_id: session.into(),
        turn_id: None,
        model: model.into(),
        provider: "test".into(),
        timestamp: at.into(),
        tokens: TokenCounts {
            input: Some(100),
            cached: Some(60),
            output: Some(20),
            reasoning: Some(10),
            cache_write: Some(10),
            total: Some(120),
        },
        service_tier: None,
        request_input: None,
        quality: "test".into(),
        price_version: Some("test-v1".into()),
        cost_nanousd: Some(3),
    }
}
fn cursor(offset: u64) -> FileCursor {
    FileCursor {
        source_id: "test".into(),
        file_key: "test".into(),
        offset,
        file_identity: "test".into(),
        modified_ns: "1".into(),
        parser_version: pulse_core::PARSER_VERSION,
        state: ParserState::default(),
    }
}
fn commit(store: &mut Store, sessions: &[SessionMeta], facts: &[UsageFact], offset: u64, tz: Tz) {
    store
        .commit_batch(&cursor(offset), sessions, facts, &[], tz)
        .unwrap();
}
fn ids(page: &SessionPage) -> Vec<String> {
    page.items.iter().map(|s| s.meta.id.clone()).collect()
}

#[test]
fn tied_session_cursors_page_in_both_directions_without_duplicates() {
    let mut store = Store::in_memory().unwrap();
    let sessions = (0..25)
        .map(|i| meta(&format!("s{i:02}"), "2026-10-01T00:00:00Z"))
        .collect::<Vec<_>>();
    commit(&mut store, &sessions, &[], 1, UTC);
    let first = store
        .session_page(&SessionPageRequest::default(), UTC)
        .unwrap();
    assert_eq!(first.items.len(), 10);
    assert!(first.newer.is_none());
    let second = store
        .session_page(
            &SessionPageRequest {
                cursor: first.older.clone(),
                ..Default::default()
            },
            UTC,
        )
        .unwrap();
    let third = store
        .session_page(
            &SessionPageRequest {
                cursor: second.older.clone(),
                ..Default::default()
            },
            UTC,
        )
        .unwrap();
    assert_eq!(third.items.len(), 5);
    assert!(third.older.is_none());
    let all = ids(&first)
        .into_iter()
        .chain(ids(&second))
        .chain(ids(&third))
        .collect::<Vec<_>>();
    assert_eq!(
        all,
        (0..25)
            .rev()
            .map(|i| format!("s{i:02}"))
            .collect::<Vec<_>>()
    );
    let back = store
        .session_page(
            &SessionPageRequest {
                cursor: third.newer,
                direction: PageDirection::Newer,
                ..Default::default()
            },
            UTC,
        )
        .unwrap();
    assert_eq!(ids(&back), ids(&second));
    let top = store
        .session_page(
            &SessionPageRequest {
                cursor: back.newer,
                direction: PageDirection::Newer,
                ..Default::default()
            },
            UTC,
        )
        .unwrap();
    assert_eq!(ids(&top), ids(&first));
    assert!(top.newer.is_none());
}

#[test]
fn model_date_scope_uses_fact_time_and_keeps_full_session_detail() {
    let mut store = Store::in_memory().unwrap();
    let old = meta("old-session", "2026-08-01T00:00:00Z");
    let other = meta("other-session", "2026-10-01T00:00:00Z");
    let events = vec![
        fact("recent-a", "old-session", "a", "2026-10-01T16:01:00Z"),
        fact("old-a", "old-session", "a", "2026-08-01T00:00:00Z"),
        fact("recent-b", "other-session", "b", "2026-10-01T16:01:00Z"),
    ];
    commit(
        &mut store,
        &[old, other],
        &events,
        1,
        chrono_tz::Asia::Shanghai,
    );
    let request = SessionPageRequest {
        filter: SessionFilter {
            model: Some("a".into()),
            from_day: Some("2026-10-02".into()),
            through_day: Some("2026-10-02".into()),
        },
        ..Default::default()
    };
    let page = store
        .session_page(&request, chrono_tz::Asia::Shanghai)
        .unwrap();
    assert_eq!(ids(&page), vec!["old-session"]);
    assert_eq!(page.items[0].total, 240); // Scope selects sessions; their detail covers the full lifetime.
    assert!(store.session_page(&request, UTC).unwrap().items.is_empty());
    assert_eq!(
        store
            .session_detail(&SessionDetailRequest {
                id: "old-session".into(),
                models_after: None,
                prices_after: None
            })
            .unwrap()
            .unwrap()
            .usage
            .events,
        2
    );
}

#[test]
fn session_details_keep_missing_counters_and_price_coverage_explicit() {
    let mut store = Store::in_memory().unwrap();
    let a = fact("a", "session", "model-a", "2026-10-01T00:00:00Z");
    let mut b = fact("b", "session", "model-b", "2026-10-02T00:00:00Z");
    b.tokens = TokenCounts {
        total: Some(50),
        ..Default::default()
    };
    b.cost_nanousd = None;
    b.price_version = None;
    commit(
        &mut store,
        &[meta("session", &b.timestamp)],
        &[a, b],
        1,
        UTC,
    );
    let detail = store
        .session_detail(&SessionDetailRequest {
            id: "session".into(),
            models_after: None,
            prices_after: None,
        })
        .unwrap()
        .unwrap();
    assert_eq!(detail.usage.events, 2);
    assert_eq!(detail.usage.total.known, 170);
    assert_eq!(detail.usage.input.known, 100);
    assert_eq!(detail.usage.input.unknown_events, 1);
    assert_eq!(detail.usage.cached.known, 60);
    assert_eq!(detail.usage.reasoning.known, 10);
    assert_eq!(detail.usage.cache_write.known, 10);
    assert_eq!(detail.usage.unpriced_tokens, 50);
    assert_eq!(detail.usage.unpriced_events, 1);
    assert_eq!(detail.usage.cost_nanousd, 3);
    assert_eq!(detail.models.len(), 2);
    assert_eq!(detail.prices.len(), 2);
    assert!(detail.prices[0].version.is_none());
    assert_eq!(detail.prices[1].version.as_deref(), Some("test-v1"));
    assert_eq!(
        detail.usage.started_at.as_deref(),
        Some("2026-10-01T00:00:00.000Z")
    );
    assert_eq!(
        detail.usage.ended_at.as_deref(),
        Some("2026-10-02T00:00:00.000Z")
    );
}

#[test]
fn model_and_price_detail_pages_are_bounded_but_all_categories_are_reachable() {
    let mut store = Store::in_memory().unwrap();
    let events = (0..63)
        .map(|i| {
            let mut f = fact(
                &format!("f{i}"),
                "session",
                &format!("m{i:02}"),
                "2026-10-01T00:00:00Z",
            );
            f.price_version = Some(format!("v{i:02}"));
            f
        })
        .collect::<Vec<_>>();
    commit(
        &mut store,
        &[meta("session", "2026-10-01T00:00:00Z")],
        &events,
        1,
        UTC,
    );
    let first = store
        .session_detail(&SessionDetailRequest {
            id: "session".into(),
            models_after: None,
            prices_after: None,
        })
        .unwrap()
        .unwrap();
    assert_eq!(first.models.len(), 50);
    assert_eq!(first.prices.len(), 50);
    let second = store
        .session_detail(&SessionDetailRequest {
            id: "session".into(),
            models_after: first.models_next,
            prices_after: first.prices_next,
        })
        .unwrap()
        .unwrap();
    assert_eq!(second.models.len(), 13);
    assert_eq!(second.prices.len(), 13);
    assert_eq!(second.models[0].model, "m50");
    assert_eq!(second.prices[0].version.as_deref(), Some("v50"));
    assert!(second.models_next.is_none() && second.prices_next.is_none());
    assert_eq!(second.usage.total.known, 63 * 120);
}

#[test]
fn empty_session_and_invalid_queries_do_not_invent_usage() {
    let mut store = Store::in_memory().unwrap();
    commit(&mut store, &[meta("empty", "")], &[], 1, UTC);
    let detail = store
        .session_detail(&SessionDetailRequest {
            id: "empty".into(),
            models_after: None,
            prices_after: None,
        })
        .unwrap()
        .unwrap();
    assert_eq!(detail.usage.events, 0);
    assert_eq!(detail.session.events, 0);
    assert_eq!(store.recent_sessions(None, 10).unwrap()[0].events, 0);
    assert!(detail.usage.started_at.is_none());
    assert!(detail.models.is_empty());
    let request = SessionPageRequest {
        direction: PageDirection::Newer,
        ..Default::default()
    };
    assert!(store.session_page(&request, UTC).is_err());
    let request = SessionPageRequest {
        filter: SessionFilter {
            from_day: Some("bad-date".into()),
            through_day: Some("z".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(store.session_page(&request, UTC).is_err());
    let request = SessionPageRequest {
        filter: SessionFilter {
            model: Some("'; DROP TABLE sessions; --".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(store.session_page(&request, UTC).unwrap().items.is_empty());
    assert_eq!(
        store
            .session_page(&SessionPageRequest::default(), UTC)
            .unwrap()
            .items
            .len(),
        1
    );
}

#[test]
fn session_timestamp_formats_sort_by_actual_instant() {
    let mut store = Store::in_memory().unwrap();
    commit(
        &mut store,
        &[
            meta("newer", "2026-10-01T00:00:00.5Z"),
            meta("older", "2026-10-01T08:00:00+08:00"),
        ],
        &[],
        1,
        UTC,
    );
    assert_eq!(
        ids(&store
            .session_page(&SessionPageRequest::default(), UTC)
            .unwrap()),
        vec!["newer", "older"]
    );
    commit(
        &mut store,
        &[meta("newer", "2026-10-01T00:00:00Z")],
        &[],
        2,
        UTC,
    );
    assert_eq!(
        store
            .session_by_id("newer")
            .unwrap()
            .unwrap()
            .meta
            .last_activity,
        "2026-10-01T00:00:00.500000000Z"
    );
}

#[test]
fn aggregate_overflow_rolls_back_facts_and_cursor_across_days_and_costs() {
    for scenario in ["daily", "session", "cost"] {
        let mut store = Store::in_memory().unwrap();
        let mut a = fact("a", "session", "m", "2026-10-01T00:00:00Z");
        let mut b = fact(
            "b",
            "session",
            "m",
            if scenario == "session" {
                "2026-10-02T00:00:00Z"
            } else {
                "2026-10-01T00:00:01Z"
            },
        );
        if scenario == "cost" {
            a.cost_nanousd = Some(i64::MAX);
            b.cost_nanousd = Some(1);
        } else {
            a.tokens = TokenCounts {
                total: Some(i64::MAX as u64),
                ..Default::default()
            };
            b.tokens = TokenCounts {
                total: Some(1),
                ..Default::default()
            };
        }
        commit(&mut store, &[meta("session", &a.timestamp)], &[a], 1, UTC);
        assert!(store.commit_batch(&cursor(2), &[], &[b], &[], UTC).is_err());
        assert_eq!(store.fact_count().unwrap(), 1);
        assert_eq!(store.cursor("test", "test").unwrap().unwrap().offset, 1);
    }
}
