use chrono::{DateTime, Duration, Utc};
use pulse_core::{
    PARSER_VERSION,
    collectors::jsonl::{read_batch, read_rates},
    domain::{OutputRate, RunSample, SessionMeta},
    ledger::ParserState,
    pricing::PriceBook,
    storage::{FileCursor, ModelSpeedRequest, SpeedRange, Store},
};
use serde_json::{Value, json};
fn at(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}
fn request(range: SpeedRange) -> ModelSpeedRequest {
    ModelSpeedRequest {
        model: "m".into(),
        from_day: "2026-10-01".into(),
        through_day: "2026-10-30".into(),
        range,
    }
}
fn run(
    id: &str,
    end: &str,
    output: u64,
    ms: u64,
    tier: Option<&str>,
    completed: bool,
) -> RunSample {
    RunSample {
        id: id.into(),
        session_id: "s".into(),
        model: "m".into(),
        started_at: (at(end) - Duration::milliseconds(ms as i64)).to_rfc3339(),
        rate: OutputRate {
            output_tokens: output,
            elapsed_ms: ms,
            measured_at: end.into(),
            service_tier: tier.map(str::to_owned),
            completed,
        },
    }
}
fn save(store: &mut Store, runs: &[RunSample], session: Option<SessionMeta>) {
    let cursor = FileCursor {
        source_id: "local".into(),
        file_key: "fixture".into(),
        offset: 0,
        file_identity: "fixture".into(),
        modified_ns: String::new(),
        parser_version: PARSER_VERSION,
        state: Default::default(),
    };
    store
        .commit_batch_with_runs(
            &cursor,
            &session.into_iter().collect::<Vec<_>>(),
            &[],
            &[],
            chrono_tz::UTC,
            runs,
        )
        .unwrap();
}
fn parse(
    p: &mut ParserState,
    kind: &str,
    payload: Value,
    ts: &str,
) -> pulse_core::ledger::ParseOutput {
    p.parse(&serde_json::to_vec(&json!({"type":kind,"payload":payload,"timestamp":ts})).unwrap())
}
fn usage(p: &mut ParserState, output: u64, ts: &str) -> pulse_core::ledger::ParseOutput {
    parse(
        p,
        "event_msg",
        json!({"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":output,"total_tokens":100+output}}}),
        ts,
    )
}
#[test]
fn weighted_mean_deduplicates_sources_and_completed_sample_wins() {
    let mut store = Store::in_memory().unwrap();
    let a = run("a", "2026-10-08T01:00:00Z", 100, 1000, Some("fast"), true);
    let b = run(
        "b",
        "2026-10-08T06:00:00Z",
        100,
        9000,
        Some("standard"),
        true,
    );
    save(&mut store, &[a.clone(), b.clone(), a.clone()], None);
    let mut stale = a.clone();
    stale.rate.completed = false;
    stale.rate.output_tokens = 1;
    save(&mut store, &[stale], None);
    let result = store
        .model_speed(
            &request(SpeedRange::Recent50),
            chrono_tz::UTC,
            at("2026-10-09T00:00:00Z"),
            &[],
        )
        .unwrap();
    assert_eq!(
        (
            result.summary.output_tokens,
            result.summary.elapsed_ms,
            result.summary.samples
        ),
        (200, 10000, 2)
    );
    assert_eq!(result.summary.service_tier.as_deref(), Some("mixed"));
    assert_eq!(result.points[0].summary.output_tokens, 100);
    assert_eq!(store.fact_count().unwrap(), 0);
}
#[test]
fn current_requires_fresh_connected_matching_run_and_does_not_borrow_another_session() {
    let mut store = Store::in_memory().unwrap();
    let sample = run("live", "2026-10-08T06:00:00Z", 200, 10000, None, false);
    let mut session = SessionMeta {
        id: "s".into(),
        last_activity: sample.rate.measured_at.clone(),
        status: "active".into(),
        current_model: Some("m".into()),
        output_rate: Some(sample.rate.clone()),
        ..Default::default()
    };
    save(
        &mut store,
        std::slice::from_ref(&sample),
        Some(session.clone()),
    );
    let query = |store: &Store, now: &str, connected: &[String]| {
        store
            .model_speed(
                &request(SpeedRange::Recent50),
                chrono_tz::UTC,
                at(now),
                connected,
            )
            .unwrap()
    };
    assert!(query(&store, "2026-10-08T06:00:01Z", &[]).current.is_none());
    assert!(
        query(&store, "2026-10-08T06:01:01Z", &["local".into()])
            .current
            .is_none()
    );
    let result = query(&store, "2026-10-08T06:00:01Z", &["local".into()]);
    assert!(result.current.is_some());
    assert_eq!(result.summary.samples, 1);
    assert!(result.summary.service_tier.is_none());
    session.output_rate.as_mut().unwrap().elapsed_ms = 10001;
    save(&mut store, &[], Some(session.clone()));
    assert!(
        query(&store, "2026-10-08T06:00:01Z", &["local".into()])
            .current
            .is_none()
    );
    session.output_rate = None;
    save(&mut store, &[], Some(session));
    assert!(
        query(&store, "2026-10-08T06:00:01Z", &["local".into()])
            .points
            .is_empty()
    );
}
#[test]
fn recent_limits_and_month_buckets_preserve_weighted_totals_and_local_dates() {
    let mut store = Store::in_memory().unwrap();
    let runs = (0..150)
        .map(|i| {
            run(
                &format!("r{i}"),
                &(at("2026-10-01T18:00:00Z") + Duration::hours(i * 4)).to_rfc3339(),
                100,
                10000,
                Some("fast"),
                true,
            )
        })
        .collect::<Vec<_>>();
    save(&mut store, &runs, None);
    for (range, count) in [(SpeedRange::Recent50, 50), (SpeedRange::Recent100, 100)] {
        let r = store
            .model_speed(
                &request(range),
                chrono_tz::Asia::Shanghai,
                at("2026-10-30T00:00:00Z"),
                &[],
            )
            .unwrap();
        assert_eq!(r.points.len(), count);
        assert_eq!(r.summary.samples, count as u64);
        assert!(
            r.points
                .windows(2)
                .all(|w| w[0].measured_at <= w[1].measured_at)
        );
    }
    let r = store
        .model_speed(
            &request(SpeedRange::Month),
            chrono_tz::Asia::Shanghai,
            at("2026-10-30T00:00:00Z"),
            &[],
        )
        .unwrap();
    assert!(r.points.len() <= 121);
    assert_eq!(r.summary.samples, 150);
    assert_eq!(r.summary.output_tokens, 15000);
    let mut local = request(SpeedRange::Recent50);
    local.from_day = "2026-10-01".into();
    local.through_day = local.from_day.clone();
    assert_eq!(
        store
            .model_speed(
                &local,
                chrono_tz::Asia::Shanghai,
                at("2026-10-30T00:00:00Z"),
                &[]
            )
            .unwrap()
            .summary
            .samples,
        0
    );
    assert_eq!(
        store
            .model_speed(&local, chrono_tz::UTC, at("2026-10-30T00:00:00Z"), &[])
            .unwrap()
            .summary
            .samples,
        2
    );
}
#[test]
fn modes_bind_to_owned_run_unknown_stays_unknown_and_switch_is_mixed() {
    let mut p = ParserState::default();
    parse(
        &mut p,
        "session_meta",
        json!({"id":"s"}),
        "2026-10-01T00:00:00Z",
    );
    parse(
        &mut p,
        "event_msg",
        json!({"type":"task_started","turn_id":"one"}),
        "2026-10-01T01:00:00Z",
    );
    parse(
        &mut p,
        "turn_context",
        json!({"model":"m","turn_id":"one","service_tier":"priority"}),
        "2026-10-01T01:00:00Z",
    );
    let a = usage(&mut p, 100, "2026-10-01T01:00:05Z")
        .run_sample
        .unwrap();
    assert_eq!(a.rate.service_tier.as_deref(), Some("fast"));
    p = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    parse(
        &mut p,
        "event_msg",
        json!({"type":"thread_settings_applied","thread_id":"foreign","thread_settings":{"model":"m","service_tier":"default"}}),
        "2026-10-01T01:00:06Z",
    );
    assert_eq!(p.rate_tier.as_deref(), Some("fast"));
    parse(
        &mut p,
        "turn_context",
        json!({"model":"m","turn_id":"one","service_tier":"default"}),
        "2026-10-01T01:00:06Z",
    );
    assert!(p.session.as_ref().unwrap().output_rate.is_none());
    let b = usage(&mut p, 200, "2026-10-01T01:00:10Z")
        .run_sample
        .unwrap();
    assert_eq!(b.id, a.id);
    assert_eq!(b.rate.service_tier.as_deref(), Some("mixed"));
    let done = parse(
        &mut p,
        "event_msg",
        json!({"type":"task_complete","turn_id":"one","duration_ms":10000}),
        "2026-10-01T01:00:10Z",
    )
    .run_sample
    .unwrap();
    assert!(done.rate.completed);
    parse(
        &mut p,
        "event_msg",
        json!({"type":"task_started","turn_id":"two"}),
        "2026-10-01T03:00:00Z",
    );
    let c = usage(&mut p, 220, "2026-10-01T03:00:01Z")
        .run_sample
        .unwrap();
    assert_eq!(c.rate.elapsed_ms, 1000);
    assert_eq!(c.rate.output_tokens, 20);
    assert!(c.rate.service_tier.is_none());
    assert_ne!(a.id, c.id);
}
#[test]
fn inherited_history_and_context_without_matching_start_never_create_speed() {
    let mut p = ParserState::default();
    parse(
        &mut p,
        "session_meta",
        json!({"id":"child","forked_from_id":"parent"}),
        "2026-10-01T01:00:00Z",
    );
    parse(
        &mut p,
        "event_msg",
        json!({"type":"task_started","turn_id":"old"}),
        "2026-10-01T00:00:00Z",
    );
    parse(
        &mut p,
        "turn_context",
        json!({"model":"m","turn_id":"old"}),
        "2026-10-01T00:00:00Z",
    );
    assert!(
        usage(&mut p, 100, "2026-10-01T00:00:01Z")
            .run_sample
            .is_none()
    );
    parse(
        &mut p,
        "event_msg",
        json!({"type":"task_started","turn_id":"new"}),
        "2026-10-01T02:00:00Z",
    );
    parse(
        &mut p,
        "turn_context",
        json!({"model":"m","turn_id":"other"}),
        "2026-10-01T02:00:00Z",
    );
    assert!(
        usage(&mut p, 200, "2026-10-01T02:00:01Z")
            .run_sample
            .is_none()
    );
}
#[test]
fn bounded_history_replay_uses_independent_cursor_and_keeps_usage_identical() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("runs.jsonl");
    let mut rows = vec![
        json!({"type":"session_meta","timestamp":"2026-10-01T00:00:00Z","payload":{"id":"s"}}),
    ];
    for i in 0..70 {
        let start = at("2026-10-08T00:00:00Z") + Duration::minutes(i * 2);
        rows.extend([
            json!({"type":"event_msg","timestamp":start.to_rfc3339(),"payload":{"type":"task_started","turn_id":format!("t{i}")}}),
            json!({"type":"turn_context","payload":{"model":"m","turn_id":format!("t{i}"),"service_tier":"fast"}}),
            json!({"type":"event_msg","timestamp":(start+Duration::seconds(10)).to_rfc3339(),"payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100*(i+1),"output_tokens":100*(i+1),"total_tokens":200*(i+1)}}}}),
            json!({"type":"event_msg","timestamp":(start+Duration::seconds(10)).to_rfc3339(),"payload":{"type":"task_complete","turn_id":format!("t{i}"),"duration_ms":10000}}),
        ]);
    }
    std::fs::write(
        &path,
        rows.iter()
            .map(|r| r.to_string() + "\n")
            .collect::<String>(),
    )
    .unwrap();
    let mut store = Store::open(temp.path().join("db.sqlite")).unwrap();
    while read_batch(
        &mut store,
        "local",
        &path,
        chrono_tz::UTC,
        &PriceBook::default(),
    )
    .unwrap()
    .more
    {}
    let count = store.fact_count().unwrap();
    let cursor = store
        .cursor("local", path.to_str().unwrap())
        .unwrap()
        .unwrap();
    let before = serde_json::to_string(&store.session_by_id("s").unwrap()).unwrap();
    // Simulate upgrade from v7: no historical run rows, but all usage checkpoints already committed.
    let raw = rusqlite::Connection::open(temp.path().join("db.sqlite")).unwrap();
    raw.execute("DELETE FROM run_speeds", []).unwrap();
    drop(raw);
    let first = read_rates(&mut store, "local", &path).unwrap();
    assert!(first.records <= 64);
    assert!(first.more);
    drop(store);
    let mut store = Store::open(temp.path().join("db.sqlite")).unwrap();
    while read_rates(&mut store, "local", &path).unwrap().more {}
    assert!(read_rates(&mut store, "local", &path).unwrap().unchanged);
    assert_eq!(store.fact_count().unwrap(), count);
    assert_eq!(
        store
            .cursor("local", path.to_str().unwrap())
            .unwrap()
            .unwrap()
            .offset,
        cursor.offset
    );
    assert_eq!(
        serde_json::to_string(&store.session_by_id("s").unwrap()).unwrap(),
        before
    );
    assert_eq!(
        store
            .model_speed(
                &request(SpeedRange::Recent100),
                chrono_tz::UTC,
                at("2026-10-09T00:00:00Z"),
                &[]
            )
            .unwrap()
            .summary
            .samples,
        70
    );
}
