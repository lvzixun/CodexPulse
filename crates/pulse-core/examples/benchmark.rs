//! Reproducible 10k-session / 100k-fact workload. Uses an isolated disposable database.
use chrono::{Duration, TimeZone, Utc};
use pulse_core::{
    PARSER_VERSION,
    domain::{SessionMeta, TokenCounts, UsageFact},
    ledger::ParserState,
    storage::{FileCursor, Store},
};
use std::{collections::BTreeMap, hint::black_box, time::Instant};

fn measure(mut operation: impl FnMut(), iterations: usize) -> serde_json::Value {
    for _ in 0..5 {
        operation();
    }
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        operation();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    serde_json::json!({ "p50_ms":samples[iterations / 2], "p95_ms":samples[(iterations * 95).div_ceil(100) - 1], "max_ms":samples[iterations - 1], "samples":iterations })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::Builder::new()
        .prefix("codexpulse-bench-")
        .tempdir()?;
    let mut store = Store::open(directory.path().join("benchmark.sqlite"))?;
    let timezone = chrono_tz::Asia::Shanghai;
    let today = Utc::now().with_timezone(&timezone).date_naive();
    let load_start = Instant::now();
    let mut loaded = 0usize;
    for batch in 0..100usize {
        let mut facts = Vec::with_capacity(1000);
        let mut sessions = BTreeMap::new();
        for offset in 0..1000usize {
            let i = batch * 1000 + offset;
            let session_id = format!("benchmark-session-{:05}", i / 10);
            let day = today - Duration::days(29 - (i % 30) as i64);
            let timestamp = Utc
                .from_utc_datetime(&day.and_hms_opt(12, 0, (i % 60) as u32).unwrap())
                .to_rfc3339();
            sessions.insert(
                session_id.clone(),
                SessionMeta {
                    id: session_id.clone(),
                    title: Some("Synthetic benchmark session".into()),
                    status: "completed".into(),
                    last_activity: timestamp.clone(),
                    ..Default::default()
                },
            );
            facts.push(UsageFact {
                id: format!("benchmark-fact-{i:06}"),
                session_id,
                turn_id: Some(format!("turn-{i}")),
                model: format!("benchmark-model-{}", i % 3),
                provider: "synthetic".into(),
                timestamp,
                tokens: TokenCounts {
                    input: Some(1000),
                    cached: Some(800),
                    output: Some(200),
                    reasoning: Some(50),
                    total: Some(1200),
                    ..Default::default()
                },
                service_tier: None,
                request_input: None,
                quality: "synthetic".into(),
                price_version: None,
                cost_nanousd: None,
            });
        }
        let cursor = FileCursor {
            source_id: "benchmark".into(),
            file_key: "synthetic".into(),
            offset: (batch as u64 + 1) * 1000,
            file_identity: "synthetic".into(),
            modified_ns: "0".into(),
            parser_version: PARSER_VERSION,
            state: ParserState::default(),
        };
        loaded += store.commit_batch(
            &cursor,
            &sessions.into_values().collect::<Vec<_>>(),
            &facts,
            &[],
            timezone,
        )?;
    }
    let load_seconds = load_start.elapsed().as_secs_f64();
    let expected = store.summary(today, timezone)?;
    assert_eq!(loaded, 100_000);
    assert_eq!(store.fact_count()?, 100_000);
    assert_eq!(expected.sessions, 10_000);
    assert_eq!(expected.total, 120_000_000);
    assert_eq!(expected.cached, 80_000_000);
    assert_eq!(expected.output, 20_000_000);
    assert_eq!(expected.days.len(), 30);
    let summary = measure(
        || {
            black_box(store.summary(today, timezone).unwrap());
        },
        100,
    );
    let recent = measure(
        || {
            black_box(store.recent_sessions(None, 10).unwrap());
        },
        100,
    );
    let database_bytes = std::fs::metadata(directory.path().join("benchmark.sqlite"))?.len();
    let page = measure(
        || {
            black_box(store.session_page(&Default::default(), timezone).unwrap());
        },
        100,
    );
    let id = store.recent_sessions(None, 1)?[0].meta.id.clone();
    let detail = measure(
        || {
            black_box(
                store
                    .session_detail(&pulse_core::storage::SessionDetailRequest {
                        id: id.clone(),
                        models_after: None,
                        prices_after: None,
                    })
                    .unwrap()
                    .unwrap(),
            );
        },
        100,
    );
    let rebuild_start = Instant::now();
    store.begin_timezone_rebuild(chrono_tz::UTC)?;
    let mut rebuild_steps = Vec::new();
    let mut query_during_rebuild_ms: f64 = 0.0;
    loop {
        let start = Instant::now();
        let progress = store.step_timezone_rebuild(256)?;
        rebuild_steps.push(start.elapsed().as_secs_f64() * 1000.0);
        if rebuild_steps.len() % 64 == 0 {
            let start = Instant::now();
            assert_eq!(store.summary(today, timezone)?.total, expected.total);
            query_during_rebuild_ms =
                query_during_rebuild_ms.max(start.elapsed().as_secs_f64() * 1000.0);
        }
        if progress.ready {
            break;
        }
    }
    let publish_start = Instant::now();
    store.commit_timezone_rebuild(&[("benchmark_timezone", serde_json::json!("UTC"))])?;
    let publish_ms = publish_start.elapsed().as_secs_f64() * 1000.0;
    let rebuild_ms = rebuild_start.elapsed().as_secs_f64() * 1000.0;
    let rebuilt = store.summary(today, chrono_tz::UTC)?;
    assert_eq!(rebuilt.total, expected.total);
    assert_eq!(rebuilt.input, expected.input);
    assert_eq!(rebuilt.cached, expected.cached);
    assert_eq!(rebuilt.output, expected.output);
    assert_eq!(rebuilt.sessions, expected.sessions);
    assert_eq!(store.fact_count()?, 100_000);
    rebuild_steps.sort_by(f64::total_cmp);
    let step_count = rebuild_steps.len();
    let rebuild = serde_json::json!({"from_timezone":timezone.name(),"to_timezone":"UTC","facts":100000,
        "total_ms":rebuild_ms,"publish_ms":publish_ms,"step_limit":256,"steps":step_count,
        "step_p50_ms":rebuild_steps[step_count/2],"step_p95_ms":rebuild_steps[(step_count*95).div_ceil(100)-1],
        "step_max_ms":rebuild_steps[step_count-1],"old_summary_query_max_ms":query_during_rebuild_ms});
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "workload": { "sessions":10000, "facts":100000, "models":3, "batch_size":1000 },
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "target_os": std::env::consts::OS,
            "logical_cpus":std::thread::available_parallelism()?.get(),
            "timezone":timezone.name(), "generated_at":Utc::now().to_rfc3339(),
            "load_seconds":load_seconds, "database_bytes":database_bytes,
            "summary_30_days":summary, "recent_10_sessions":recent,
            "session_page":page, "session_detail":detail,
            "timezone_rebuild":rebuild,
            "correctness": "100k unique facts, 10k distinct sessions, cached input counted once, 30 calendar buckets verified"
        }))?
    );
    Ok(())
}
