use pulse_core::{
    domain::*,
    ledger::ParserState,
    pricing::{Price, PriceBook},
    storage::{FileCursor, Store},
};
use serde_json::{Value, json};
fn row(kind: &str, payload: Value, ts: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"type":kind,"payload":payload,"timestamp":ts})).unwrap()
}
fn counts(input: u64, cached: u64, output: u64) -> Value {
    json!({"input_tokens":input,"cached_input_tokens":cached,"output_tokens":output,"total_tokens":input+output})
}
fn state(id: &str) -> ParserState {
    let mut p = ParserState::default();
    p.parse(&row(
        "session_meta",
        json!({"id":id,"model_provider":"openai","cwd":"D:/project","cli_version":"0.160.0"}),
        "2026-10-01T00:00:00Z",
    ));
    model(&mut p, "a");
    p
}
fn model(p: &mut ParserState, m: &str) {
    p.parse(&row(
        "turn_context",
        json!({"model":m,"turn_id":"turn"}),
        "2026-10-01T00:00:00Z",
    ));
}
fn usage(p: &mut ParserState, c: Value, last: Value, ts: &str) -> Option<UsageFact> {
    p.parse(&row(
        "event_msg",
        json!({"type":"token_count","info":{"total_token_usage":c,"last_token_usage":last}}),
        ts,
    ))
    .fact
}
fn cursor(state: ParserState, source: &str) -> FileCursor {
    FileCursor {
        source_id: source.into(),
        file_key: "session.jsonl".into(),
        offset: 100,
        file_identity: "identity".into(),
        modified_ns: "1".into(),
        parser_version: 1,
        state,
    }
}

#[test]
fn cumulative_counts_and_model_switches_are_incremental() {
    let mut p = state("session");
    let first = usage(
        &mut p,
        counts(100, 60, 10),
        counts(100, 60, 10),
        "2026-10-01T00:01:00Z",
    )
    .unwrap();
    assert_eq!(first.tokens.effective_total(), Some(110));
    assert!(
        usage(
            &mut p,
            counts(100, 60, 10),
            counts(100, 60, 10),
            "2026-10-01T00:01:01Z"
        )
        .is_none()
    );
    model(&mut p, "b");
    let second = usage(
        &mut p,
        counts(200, 120, 20),
        counts(100, 60, 10),
        "2026-10-01T00:02:00Z",
    )
    .unwrap();
    assert_eq!(second.model, "b");
    assert_eq!(second.tokens.input, Some(100));
    model(&mut p, "a");
    let third = usage(
        &mut p,
        counts(300, 180, 30),
        counts(100, 60, 10),
        "2026-10-01T00:03:00Z",
    )
    .unwrap();
    assert_eq!(third.tokens.input, Some(100));
    assert_eq!(third.model, "a");
}
#[test]
fn mirror_and_archive_replays_deduplicate_and_sessions_are_distinct() {
    let mut store = Store::in_memory().unwrap();
    let mut p = state("same");
    let fact = usage(
        &mut p,
        counts(100, 60, 10),
        counts(100, 60, 10),
        "2026-10-01T01:00:00Z",
    )
    .unwrap();
    let meta = p.session.clone().unwrap();
    assert_eq!(
        store
            .commit_batch(
                &cursor(p.clone(), "windows"),
                std::slice::from_ref(&meta),
                std::slice::from_ref(&fact),
                &[],
                chrono_tz::Asia::Shanghai
            )
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .commit_batch(
                &cursor(p.clone(), "wsl"),
                std::slice::from_ref(&meta),
                std::slice::from_ref(&fact),
                &[],
                chrono_tz::Asia::Shanghai
            )
            .unwrap(),
        0
    );
    let mut archived = cursor(p.clone(), "windows");
    archived.file_key = "archived.jsonl".into();
    store
        .commit_batch(&archived, &[meta], &[fact], &[], chrono_tz::Asia::Shanghai)
        .unwrap();
    model(&mut p, "b");
    let other = usage(
        &mut p,
        counts(200, 120, 20),
        counts(100, 60, 10),
        "2026-10-01T02:00:00Z",
    )
    .unwrap();
    store
        .commit_batch(
            &cursor(p.clone(), "windows"),
            &[p.session.clone().unwrap()],
            &[other],
            &[],
            chrono_tz::Asia::Shanghai,
        )
        .unwrap();
    assert_eq!(store.fact_count().unwrap(), 2);
    let models = store
        .model_usage("2026-10-01", "2026-10-01", chrono_tz::Asia::Shanghai)
        .unwrap();
    assert_eq!(models.iter().map(|m| m.total).sum::<u64>(), 220);
    assert_eq!(models.iter().map(|m| m.sessions).sum::<u64>(), 2);
    assert_eq!(
        store
            .independent_sessions("2026-10-01", "2026-10-01", chrono_tz::Asia::Shanghai)
            .unwrap(),
        1
    );
}
#[test]
fn fork_initial_snapshot_is_inherited_not_new_spend() {
    let mut p = ParserState::default();
    p.parse(&row(
        "session_meta",
        json!({"id":"child","forked_from_id":"parent"}),
        "2026-10-01T00:00:00Z",
    ));
    assert!(
        usage(
            &mut p,
            counts(100, 60, 10),
            counts(0, 0, 0),
            "2026-10-01T00:01:00Z"
        )
        .is_none()
    );
    let owned = usage(
        &mut p,
        counts(120, 65, 15),
        counts(20, 5, 5),
        "2026-10-01T00:02:00Z",
    )
    .unwrap();
    assert_eq!(owned.tokens.effective_total(), Some(25));
}
#[test]
fn fork_without_parent_log_can_use_explicit_last_usage() {
    let mut p = ParserState::default();
    p.parse(&row(
        "session_meta",
        json!({"id":"page","history_base":{"thread_id":"older-page"}}),
        "2026-10-01T00:00:00Z",
    ));
    let fact = usage(
        &mut p,
        counts(120, 65, 15),
        counts(20, 5, 5),
        "2026-10-01T00:02:00Z",
    )
    .unwrap();
    assert_eq!(fact.tokens.effective_total(), Some(25));
}
#[test]
fn reasoning_and_cached_input_are_not_added_twice() {
    let t = TokenCounts {
        input: Some(100),
        cached: Some(60),
        output: Some(10),
        reasoning: Some(8),
        total: Some(110),
        ..Default::default()
    };
    assert!(t.validate().is_ok());
    assert_eq!(t.effective_total(), Some(110));
}
#[test]
fn unknown_breakdown_keeps_total_and_remains_unpriced() {
    let mut p = state("session");
    let fact = usage(
        &mut p,
        json!({"total_tokens":999}),
        Value::Null,
        "2026-10-01T00:01:00Z",
    )
    .unwrap();
    assert_eq!(fact.tokens.input, None);
    assert_eq!(fact.tokens.effective_total(), Some(999));
    assert_eq!(fact.quality, "unsplit");
    assert!(PriceBook::default().price(&fact).is_none());
}
#[test]
fn invalid_cache_and_overflow_are_rejected() {
    let mut p = state("bad");
    let result = p.parse(&row(
        "event_msg",
        json!({"type":"token_count","info":{"total_token_usage":counts(10,20,1)}}),
        "2026-10-01T00:01:00Z",
    ));
    assert!(result.fact.is_none());
    assert_eq!(result.issue.unwrap().code, "invalid_token_counters");
    assert!(
        TokenCounts {
            input: Some(u64::MAX),
            output: Some(1),
            ..Default::default()
        }
        .validate()
        .is_err()
    );
}
#[test]
fn pricing_uses_fixed_point_cached_subset_and_tier() {
    let book = PriceBook {
        prices: vec![Price {
            version: "test-price".into(),
            provider: "openai".into(),
            model: "a".into(),
            service_tier: "standard".into(),
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
            max_input: None,
            input_microusd: 2_000_000,
            cached_microusd: 200_000,
            output_microusd: 10_000_000,
            source_url: "https://example.invalid/test-only".into(),
            checked_at: "2026-10-06".into(),
        }],
    };
    let mut p = state("priced");
    let mut fact = usage(
        &mut p,
        counts(100_000, 60_000, 10_000),
        counts(100_000, 60_000, 10_000),
        "2026-10-01T00:01:00Z",
    )
    .unwrap();
    book.apply(&mut fact);
    assert_eq!(fact.cost_nanousd, Some(192_000_000));
    fact.service_tier = Some("priority".into());
    assert!(book.price(&fact).is_none());
    fact.service_tier = None;
    fact.tokens.cached = None;
    assert!(book.price(&fact).is_none());
}
#[test]
fn calendar_buckets_use_event_time_not_session_creation() {
    let mut store = Store::in_memory().unwrap();
    let mut p = state("spans-midnight");
    let a = usage(
        &mut p,
        counts(100, 60, 10),
        counts(100, 60, 10),
        "2026-10-01T15:59:59Z",
    )
    .unwrap();
    let b = usage(
        &mut p,
        counts(200, 120, 20),
        counts(100, 60, 10),
        "2026-10-01T16:00:01Z",
    )
    .unwrap();
    store
        .commit_batch(
            &cursor(p.clone(), "source"),
            &[p.session.unwrap()],
            &[a, b],
            &[],
            chrono_tz::Asia::Shanghai,
        )
        .unwrap();
    assert_eq!(
        store
            .model_usage("2026-10-01", "2026-10-01", chrono_tz::Asia::Shanghai)
            .unwrap()[0]
            .total,
        110
    );
    assert_eq!(
        store
            .model_usage("2026-10-02", "2026-10-02", chrono_tz::Asia::Shanghai)
            .unwrap()[0]
            .total,
        110
    );
}
#[test]
fn rejected_transaction_does_not_advance_checkpoint() {
    let mut store = Store::in_memory().unwrap();
    let mut p = state("valid");
    let mut fact = usage(
        &mut p,
        counts(100, 60, 10),
        counts(100, 60, 10),
        "2026-10-01T00:01:00Z",
    )
    .unwrap();
    fact.session_id = "no-such-session".into();
    let checkpoint = cursor(p.clone(), "source");
    assert!(
        store
            .commit_batch(
                &checkpoint,
                &[p.session.unwrap()],
                &[fact],
                &[],
                chrono_tz::UTC
            )
            .is_err()
    );
    assert_eq!(store.fact_count().unwrap(), 0);
    assert!(store.cursor("source", "session.jsonl").unwrap().is_none());
}
#[test]
fn crash_checkpoint_roundtrip_keeps_cumulative_baseline() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pulse.db");
    let mut store = Store::open(&path).unwrap();
    let mut p = state("restart");
    let a = usage(
        &mut p,
        counts(100, 60, 10),
        counts(100, 60, 10),
        "2026-10-01T00:01:00Z",
    )
    .unwrap();
    store
        .commit_batch(
            &cursor(p.clone(), "source"),
            &[p.session.unwrap()],
            &[a],
            &[],
            chrono_tz::UTC,
        )
        .unwrap();
    drop(store);
    let store = Store::open(path).unwrap();
    let mut resumed = store
        .cursor("source", "session.jsonl")
        .unwrap()
        .unwrap()
        .state;
    let b = usage(
        &mut resumed,
        counts(200, 120, 20),
        counts(100, 60, 10),
        "2026-10-01T00:02:00Z",
    )
    .unwrap();
    assert_eq!(b.tokens.effective_total(), Some(110));
}

#[test]
fn decreasing_snapshot_does_not_replay_last_usage() {
    let mut p = state("correction");
    usage(
        &mut p,
        counts(200, 120, 20),
        counts(100, 60, 10),
        "2026-10-01T00:01:00Z",
    )
    .unwrap();
    let result=p.parse(&row("event_msg",json!({"type":"token_count","info":{"total_token_usage":counts(100,60,10),"last_token_usage":counts(100,60,10)}}),"2026-10-01T00:02:00Z"));
    assert!(result.fact.is_none());
    assert_eq!(result.issue.unwrap().code, "usage_ownership_unresolved");
    assert_eq!(
        usage(
            &mut p,
            counts(120, 65, 15),
            counts(20, 5, 5),
            "2026-10-01T00:03:00Z"
        )
        .unwrap()
        .tokens
        .effective_total(),
        Some(25)
    );
}
#[test]
fn negative_counters_are_rejected_instead_of_becoming_unknown() {
    let mut p = state("negative");
    let result=p.parse(&row("event_msg",json!({"type":"token_count","info":{"total_token_usage":{"total_tokens":100,"cached_input_tokens":-1}}}),"2026-10-01T00:01:00Z"));
    assert!(result.fact.is_none());
    assert_eq!(result.issue.unwrap().code, "invalid_token_counters");
}
#[test]
fn timestamp_offsets_do_not_change_duplicate_identity() {
    let mut a = state("mirror");
    let mut b = state("mirror");
    let fa = usage(
        &mut a,
        counts(100, 60, 10),
        counts(100, 60, 10),
        "2026-10-01T00:01:00Z",
    )
    .unwrap();
    let fb = usage(
        &mut b,
        counts(100, 60, 10),
        counts(100, 60, 10),
        "2026-10-01T08:01:00+08:00",
    )
    .unwrap();
    assert_eq!(fa.id, fb.id);
}

#[test]
fn optional_field_appearing_does_not_erase_known_total_delta() {
    let mut p = state("evolving-schema");
    usage(
        &mut p,
        json!({"input_tokens":100,"output_tokens":10,"total_tokens":110}),
        Value::Null,
        "2026-10-01T00:01:00Z",
    )
    .unwrap();
    let next = usage(
        &mut p,
        counts(120, 5, 15),
        Value::Null,
        "2026-10-01T00:02:00Z",
    )
    .unwrap();
    assert_eq!(next.tokens.effective_total(), Some(25));
    assert_eq!(next.tokens.cached, None);
}
