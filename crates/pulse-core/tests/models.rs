use chrono::NaiveDate;
use chrono_tz::{Tz, UTC};
use pulse_core::{
    domain::{SessionMeta, TokenCounts, UsageFact},
    ledger::ParserState,
    storage::*,
};
use std::collections::BTreeSet;

fn request() -> ModelPageRequest {
    ModelPageRequest {
        from_day: "2026-10-01".into(),
        through_day: "2026-10-30".into(),
        ..Default::default()
    }
}
fn fact(id: &str, model: &str, at: &str, total: u64) -> UsageFact {
    UsageFact {
        id: id.into(),
        session_id: "shared-session".into(),
        turn_id: None,
        model: model.into(),
        provider: "test".into(),
        timestamp: at.into(),
        tokens: TokenCounts {
            input: Some(total - 2),
            cached: Some(3.min(total - 2)),
            output: Some(2),
            total: Some(total),
            ..Default::default()
        },
        service_tier: None,
        request_input: None,
        quality: "test".into(),
        price_version: Some("historical".into()),
        cost_nanousd: Some(7),
    }
}
fn commit(store: &mut Store, facts: &[UsageFact], tz: Tz) {
    let cursor = FileCursor {
        source_id: "test".into(),
        file_key: "test".into(),
        offset: store.fact_count().unwrap() + facts.len() as u64,
        file_identity: "test".into(),
        modified_ns: "1".into(),
        parser_version: pulse_core::PARSER_VERSION,
        state: ParserState::default(),
    };
    let session = SessionMeta {
        id: "shared-session".into(),
        last_activity: "2026-10-30T00:00:00Z".into(),
        ..Default::default()
    };
    store
        .commit_batch(&cursor, &[session], facts, &[], tz)
        .unwrap();
}
fn collect(store: &Store, mut request: ModelPageRequest, tz: Tz) -> Vec<ModelRow> {
    let mut result = Vec::new();
    loop {
        let page = store.model_page(&request, tz).unwrap();
        assert!(page.items.len() <= 20);
        result.extend(page.items);
        match page.next {
            Some(cursor) => {
                request = ModelPageRequest {
                    cursor: Some(cursor),
                    direction: ModelDirection::Next,
                    ..request
                }
            }
            None => break,
        }
    }
    result
}
#[test]
fn every_model_is_reachable_and_reconciles_with_bounded_summary() {
    let mut store = Store::in_memory().unwrap();
    let mut facts = (0..263)
        .map(|n| {
            fact(
                &format!("f{n}"),
                &format!("m{n:03}"),
                "2026-10-06T00:00:00Z",
                12,
            )
        })
        .collect::<Vec<_>>();
    let mut unknown = fact("unknown", "unknown", "2026-10-06T00:00:00Z", 14);
    unknown.cost_nanousd = None;
    unknown.tokens.input = None;
    unknown.tokens.cached = None;
    unknown.tokens.output = None;
    facts.push(unknown);
    facts.push(fact("unicode", "模型 ' ?", "2026-10-06T00:00:00Z", 13));
    facts.push(fact("outside", "old-only", "2026-08-01T00:00:00Z", 99));
    commit(&mut store, &facts, UTC);
    let summary = store
        .summary(NaiveDate::from_ymd_opt(2026, 10, 30).unwrap(), UTC)
        .unwrap();
    let models = collect(&store, request(), UTC);
    assert_eq!(models.len(), 265);
    assert_eq!(
        models
            .iter()
            .map(|m| m.usage.model.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        265
    );
    assert_eq!(summary.model_count, 265);
    assert_eq!(summary.sessions, 1);
    assert_eq!(models.iter().map(|m| m.usage.sessions).sum::<u64>(), 265);
    macro_rules! sum {
        ($field:ident) => {
            assert_eq!(
                models.iter().map(|m| m.usage.$field).sum::<u64>(),
                summary.$field
            );
        };
    }
    sum!(total);
    sum!(input);
    sum!(cached);
    sum!(output);
    sum!(unpriced_tokens);
    sum!(incomplete_events);
    assert_eq!(
        models.iter().map(|m| m.usage.cost_nanousd).sum::<i64>(),
        summary.cost_nanousd
    );
    let json = serde_json::to_value(summary).unwrap();
    assert!(json.get("models").is_none());
    assert_eq!(json["days"].as_array().unwrap().len(), 30);
    assert_eq!(models[0].usage.model, "unknown");
    assert_eq!(models[0].usage.unpriced_tokens, 14);
    assert_eq!(models[0].usage.incomplete_events, 1);
}
#[test]
fn tied_sort_keys_page_both_ways_and_freeze_live_appends_across_restart() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("pulse.sqlite");
    let mut store = Store::open(&database).unwrap();
    commit(
        &mut store,
        &(0..63)
            .map(|n| {
                fact(
                    &format!("f{n}"),
                    &format!("m{n:02}"),
                    "2026-10-06T00:00:00Z",
                    12,
                )
            })
            .collect::<Vec<_>>(),
        UTC,
    );
    let first = store.model_page(&request(), UTC).unwrap();
    let second_request = ModelPageRequest {
        cursor: first.next.clone(),
        ..request()
    };
    let second = store.model_page(&second_request, UTC).unwrap();
    let previous = store
        .model_page(
            &ModelPageRequest {
                cursor: second.previous,
                direction: ModelDirection::Previous,
                ..request()
            },
            UTC,
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(previous).unwrap(),
        serde_json::to_value(&first).unwrap()
    );
    commit(
        &mut store,
        &[
            fact("append-existing", "m60", "2026-10-06T01:00:00Z", 1000),
            fact("append-new", "new", "2026-10-06T01:00:00Z", 1000),
        ],
        UTC,
    );
    assert_ne!(store.fact_revision().unwrap(), first.watermark);
    drop(store);
    let store = Store::open(&database).unwrap();
    let mut frozen = first.items;
    frozen.extend(collect(&store, second_request, UTC));
    assert_eq!(frozen.len(), 63);
    assert_eq!(frozen.iter().map(|m| m.usage.total).sum::<u64>(), 63 * 12);
    assert_eq!(
        frozen
            .iter()
            .map(|m| m.usage.model.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        63
    );
    let refreshed = collect(&store, request(), UTC);
    assert_eq!(refreshed.len(), 64);
    assert_eq!(refreshed[0].usage.model, "m60");
    assert_eq!(refreshed[0].usage.total, 1012);
    assert_eq!(refreshed[1].usage.model, "new");
}
#[test]
fn cursor_preserves_large_integer_order_and_rejects_stale_or_malformed_scope() {
    let mut store = Store::in_memory().unwrap();
    let total = (1u64 << 53) + 1;
    commit(
        &mut store,
        &(0..21)
            .map(|n| {
                fact(
                    &format!("f{n}"),
                    &format!("m{n:02}"),
                    "2026-10-06T00:00:00Z",
                    total,
                )
            })
            .collect::<Vec<_>>(),
        UTC,
    );
    let first = store.model_page(&request(), UTC).unwrap();
    let cursor = first.next.unwrap();
    assert_eq!(cursor.total, total.to_string());
    let next = ModelPageRequest {
        cursor: Some(cursor.clone()),
        ..request()
    };
    assert_eq!(
        store.model_page(&next, UTC).unwrap().items[0].usage.model,
        "m20"
    );
    assert!(store.model_page(&next, chrono_tz::Asia::Shanghai).is_err());
    for invalid in ["-1", "1e6", "NaN", "9223372036854775808"] {
        let mut next = next.clone();
        next.cursor.as_mut().unwrap().total = invalid.into();
        assert!(store.model_page(&next, UTC).is_err());
    }
    let mut next = next;
    next.through_day = "2026-10-29".into();
    assert!(store.model_page(&next, UTC).is_err());
    next = request();
    next.cursor = Some(ModelCursor {
        watermark: "9999".into(),
        ..cursor
    });
    assert!(store.model_page(&next, UTC).is_err());
    for (from, to) in [
        ("bad-date", "2026-10-30"),
        ("2026-10-31", "2026-10-30"),
        ("2025-01-01", "2026-10-30"),
    ] {
        next = ModelPageRequest {
            from_day: from.into(),
            through_day: to.into(),
            ..Default::default()
        };
        assert!(store.model_page(&next, UTC).is_err());
    }
    assert!(
        store
            .model_page(
                &ModelPageRequest {
                    direction: ModelDirection::Previous,
                    ..request()
                },
                UTC
            )
            .is_err()
    );
}
#[test]
fn calendar_range_is_fact_time_and_unknown_events_do_not_disappear() {
    let mut store = Store::in_memory().unwrap();
    let mut unknown = fact("unknown", "unknown", "2026-10-05T16:00:00Z", 12);
    unknown.tokens = TokenCounts::default();
    unknown.cost_nanousd = None;
    commit(
        &mut store,
        &[
            fact("before", "m", "2026-10-05T15:59:59Z", 12),
            unknown,
            fact("in", "m", "2026-10-06T15:59:59Z", 12),
            fact("after", "m", "2026-10-06T16:00:00Z", 12),
        ],
        chrono_tz::Asia::Shanghai,
    );
    let scope = ModelPageRequest {
        from_day: "2026-10-06".into(),
        through_day: "2026-10-06".into(),
        ..Default::default()
    };
    let page = store.model_page(&scope, chrono_tz::Asia::Shanghai).unwrap();
    assert_eq!(page.total_models, 2);
    assert_eq!(page.items[0].usage.total, 12);
    assert_eq!(page.items[1].usage.total, 0);
    assert_eq!(page.items[1].unknown_totals, 1);
    assert_eq!(page.items[1].unknown_input, 1);
    assert_eq!(page.items[1].unpriced_events, 1);
    assert_eq!(page.items[1].usage.incomplete_events, 1);
    assert_eq!(page.items[1].usage.sessions, 1);
    assert!(page.next.is_none() && page.previous.is_none());
    let empty = Store::in_memory()
        .unwrap()
        .model_page(&request(), UTC)
        .unwrap();
    assert!(empty.items.is_empty());
    assert_eq!(empty.watermark, "0");
}
