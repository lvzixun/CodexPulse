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
            "correctness": "100k unique facts, 10k distinct sessions, cached input counted once, 30 calendar buckets verified"
        }))?
    );
    Ok(())
}
