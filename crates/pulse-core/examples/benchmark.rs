//! Reproducible 10k-session / 100k-fact workload. Uses an isolated disposable database.
use chrono::{Duration, TimeZone, Utc};
use pulse_core::{
    PARSER_VERSION,
    domain::{SessionMeta, TokenCounts, UsageFact},
    ledger::ParserState,
    pricing::{Price, PriceBook},
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
    let model_count = std::env::var("PULSE_BENCH_MODELS")
        .ok()
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(3);
    if !(1..=10_000).contains(&model_count) {
        return Err("PULSE_BENCH_MODELS must be between 1 and 10000".into());
    }
    // Synthetic rates only: exercise both priced and unknown projections without
    // tying performance measurements to changing product prices or user data.
    let book = PriceBook {
        prices: (0..model_count)
            .map(|i| Price {
                version: "synthetic-reference-v1".into(),
                provider: "synthetic".into(),
                model: format!("benchmark-model-{i:04}"),
                service_tier: "standard".into(),
                effective_from: "2020-01-01T00:00:00Z".into(),
                effective_to: None,
                min_input: None,
                max_input: None,
                input_microusd: 2_000_000,
                cached_microusd: 200_000,
                cache_write_microusd: Some(2_500_000),
                output_microusd: 10_000_000,
                source_url: "https://example.invalid/synthetic-benchmark".into(),
                checked_at: today.to_string(),
            })
            .collect(),
    };
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
                model: format!("benchmark-model-{:04}", i % model_count),
                provider: "synthetic".into(),
                timestamp,
                tokens: TokenCounts {
                    input: Some(1000),
                    cached: Some(800),
                    output: Some(200),
                    reasoning: Some(50),
                    total: Some(1200),
                    cache_write: Some(0),
                },
                service_tier: (i % 10 != 0).then(|| "standard".into()),
                request_input: Some(1000),
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
    assert!(store.summary(today, timezone)?.reference.pending);
    let projection_start = Instant::now();
    let mut projection_steps = Vec::new();
    let mut projected = 0;
    loop {
        let start = Instant::now();
        let changed = store.step_reference_prices(&book, 1024)?;
        if changed == 0 {
            break;
        }
        projection_steps.push(start.elapsed().as_secs_f64() * 1000.0);
        projected += changed;
    }
    let projection_ms = projection_start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(projected, 100_000);
    let expected = store.summary(today, timezone)?;
    assert_eq!(loaded, 100_000);
    assert_eq!(store.fact_count()?, 100_000);
    assert_eq!(expected.sessions, 10_000);
    assert_eq!(expected.total, 120_000_000);
    assert_eq!(expected.cached, 80_000_000);
    assert_eq!(expected.output, 20_000_000);
    assert_eq!(expected.days.len(), 30);
    assert_eq!(expected.model_count, model_count as u64);
    assert!(!expected.reference.pending);
    // (200 uncached * 2 + 800 cached * .2 + 200 output * 10) / million.
    assert_eq!(expected.reference.cost_nanousd, 230_400_000_000);
    assert_eq!(expected.reference.unpriced_events, 10_000);
    assert_eq!(expected.reference.unpriced_tokens, 12_000_000);
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
    let model_request = pulse_core::storage::ModelPageRequest {
        from_day: expected.from_day.clone(),
        through_day: expected.through_day.clone(),
        ..Default::default()
    };
    let model_page = measure(
        || {
            black_box(store.model_page(&model_request, timezone).unwrap());
        },
        100,
    );
    let first_models = store.model_page(&model_request, timezone)?;
    let first_model_page_bytes = serde_json::to_vec(&first_models)?.len();
    let summary_bytes = serde_json::to_vec(&expected)?.len();
    let mut model_request_next = model_request.clone();
    let mut reached_models = 0usize;
    let mut reached_tokens = 0u64;
    let mut reached_cost = 0i64;
    let mut pages = 0usize;
    loop {
        let model_page = store.model_page(&model_request_next, timezone)?;
        assert!(model_page.items.len() <= 20);
        reached_models += model_page.items.len();
        reached_tokens += model_page.items.iter().map(|m| m.usage.total).sum::<u64>();
        assert!(model_page.items.iter().all(|m| !m.reference.pending));
        reached_cost += model_page
            .items
            .iter()
            .map(|m| m.reference.cost_nanousd)
            .sum::<i64>();
        pages += 1;
        match model_page.next {
            Some(cursor) => model_request_next.cursor = Some(cursor),
            None => break,
        }
    }
    assert_eq!(reached_models, model_count);
    assert_eq!(reached_tokens, expected.total);
    assert_eq!(reached_cost, expected.reference.cost_nanousd);
    let database_bytes = std::fs::metadata(directory.path().join("benchmark.sqlite"))?.len();
    let page = measure(
        || {
            black_box(store.session_page(&Default::default(), timezone).unwrap());
        },
        100,
    );
    let id = store.recent_sessions(None, 1)?[0].meta.id.clone();
    let activity_page = measure(
        || {
            black_box(
                store
                    .session_page_with_activity(
                        &Default::default(),
                        timezone,
                        Utc::now(),
                        &["benchmark".into()],
                    )
                    .unwrap(),
            );
        },
        100,
    );
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
    assert_eq!(
        rebuilt.reference.cost_nanousd,
        expected.reference.cost_nanousd
    );
    rebuild_steps.sort_by(f64::total_cmp);
    let step_count = rebuild_steps.len();
    let rebuild = serde_json::json!({"from_timezone":timezone.name(),"to_timezone":"UTC","facts":100000,
        "total_ms":rebuild_ms,"publish_ms":publish_ms,"step_limit":256,"steps":step_count,
        "step_p50_ms":rebuild_steps[step_count/2],"step_p95_ms":rebuild_steps[(step_count*95).div_ceil(100)-1],
        "step_max_ms":rebuild_steps[step_count-1],"old_summary_query_max_ms":query_during_rebuild_ms});
    projection_steps.sort_by(f64::total_cmp);
    let projection = serde_json::json!({"facts":projected,"total_ms":projection_ms,"steps":projection_steps.len(),
        "step_limit":1024,"step_p95_ms":projection_steps[(projection_steps.len()*95).div_ceil(100)-1],
        "step_max_ms":projection_steps[projection_steps.len()-1],"priced_facts":90000,"unknown_facts":10000,
        "cost_nanousd":expected.reference.cost_nanousd,"prices":"synthetic"});
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "workload": { "sessions":10000, "facts":100000, "models":model_count, "batch_size":1000 },
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "target_os": std::env::consts::OS,
            "logical_cpus":std::thread::available_parallelism()?.get(),
            "timezone":timezone.name(), "generated_at":Utc::now().to_rfc3339(),
            "load_seconds":load_seconds, "database_bytes":database_bytes,
            "summary_30_days":summary, "recent_10_sessions":recent,
            "session_page":page, "session_page_activity":activity_page, "session_detail":detail,
            "model_page":model_page,
            "model_paging": { "pages":pages, "reached_models":reached_models, "page_limit":20, "summary_json_bytes":summary_bytes, "first_page_json_bytes":first_model_page_bytes },
            "timezone_rebuild":rebuild,
            "reference_projection":projection,
            "correctness": "100k unique facts, 10k distinct sessions, cached input counted once, 30 calendar buckets verified"
        }))?
    );
    Ok(())
}
