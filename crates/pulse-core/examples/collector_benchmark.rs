//! Bounded JSONL collection benchmarks. Generates only synthetic logs in a temporary directory.
//! Run `collector_benchmark history` or `collector_benchmark large <MiB>` in release mode.
use chrono::{DateTime, Utc};
use pulse_core::{collectors::jsonl::read_batch, pricing::PriceBook, storage::Store};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
    time::Instant,
};

#[derive(Default, Serialize)]
struct ReadTotals {
    bytes: u64,
    records: usize,
    inserted: usize,
    issues: usize,
    batches: usize,
    max_batch_bytes: usize,
    max_batch_ms: f64,
}

fn collect(
    store: &mut Store,
    source: &str,
    path: &Path,
) -> Result<ReadTotals, Box<dyn std::error::Error>> {
    let mut totals = ReadTotals::default();
    let prices = PriceBook::default();
    loop {
        let start = Instant::now();
        let report = read_batch(store, source, path, chrono_tz::UTC, &prices)?;
        totals.max_batch_ms = totals
            .max_batch_ms
            .max(start.elapsed().as_secs_f64() * 1000.);
        totals.bytes += report.bytes_read as u64;
        totals.records += report.records;
        totals.inserted += report.inserted;
        totals.issues += report.issues;
        totals.batches += 1;
        totals.max_batch_bytes = totals.max_batch_bytes.max(report.bytes_read);
        assert!(report.bytes_read <= 8 * 1024 * 1024);
        assert!(report.records <= 256);
        if !report.more {
            break;
        }
        assert!(
            report.bytes_read > 0,
            "collector must advance between bounded batches"
        );
    }
    assert_eq!(totals.issues, 0);
    Ok(totals)
}

fn record(writer: &mut impl Write, value: Value) -> std::io::Result<()> {
    serde_json::to_writer(&mut *writer, &value)?;
    writer.write_all(b"\n")
}

fn header(writer: &mut impl Write, id: &str, timestamp: &str) -> std::io::Result<()> {
    record(
        writer,
        json!({"type":"session_meta","timestamp":timestamp,
        "payload":{"id":id,"model_provider":"synthetic"}}),
    )?;
    record(
        writer,
        json!({"type":"turn_context","payload":{"model":"benchmark-model","turn_id":"synthetic-turn"}}),
    )
}

fn usage(writer: &mut impl Write, event: u64, timestamp: &str) -> std::io::Result<()> {
    record(
        writer,
        json!({"type":"event_msg","timestamp":timestamp,
        "payload":{"type":"token_count","info":{"total_token_usage":{
            "input_tokens":1000 * event,"cached_input_tokens":800 * event,
            "output_tokens":200 * event,"total_tokens":1200 * event}}}}),
    )
}

fn history(directory: &Path, now: DateTime<Utc>) -> Result<Value, Box<dyn std::error::Error>> {
    let logs = directory.join("sessions");
    std::fs::create_dir(&logs)?;
    let timestamp = now.to_rfc3339();
    let generate = Instant::now();
    let mut bytes = 0u64;
    for n in 0..10_000 {
        let path = logs.join(format!("session-{n:05}.jsonl"));
        let mut writer = BufWriter::new(File::create(&path)?);
        header(
            &mut writer,
            &format!("benchmark-session-{n:05}"),
            &timestamp,
        )?;
        for event in 1..=10 {
            usage(&mut writer, event, &timestamp)?;
        }
        writer.flush()?;
        bytes += path.metadata()?.len();
    }
    let generation_seconds = generate.elapsed().as_secs_f64();
    let mut store = Store::open(directory.join("history.sqlite"))?;
    let mut passes = Vec::new();
    for (source, expected_insertions) in [("macos", 100_000), ("mirror", 0)] {
        let start = Instant::now();
        let mut read_bytes = 0u64;
        let mut inserted = 0;
        let mut max_batch_ms = 0f64;
        for n in 0..10_000 {
            let totals = collect(
                &mut store,
                source,
                &logs.join(format!("session-{n:05}.jsonl")),
            )?;
            read_bytes += totals.bytes;
            inserted += totals.inserted;
            max_batch_ms = max_batch_ms.max(totals.max_batch_ms);
        }
        assert_eq!(read_bytes, bytes);
        assert_eq!(inserted, expected_insertions);
        passes.push(
            json!({"source":source,"seconds":start.elapsed().as_secs_f64(),
            "bytes_read":read_bytes,"inserted":inserted,"max_batch_ms":max_batch_ms}),
        );
    }
    let summary = store.summary(now.date_naive(), chrono_tz::UTC)?;
    assert_eq!(store.fact_count()?, 100_000);
    assert_eq!(summary.sessions, 10_000);
    assert_eq!(summary.total, 120_000_000);
    assert_eq!(summary.cached, 80_000_000);
    let unchanged = Instant::now();
    for source in ["macos", "mirror"] {
        for n in 0..10_000 {
            let report = read_batch(
                &mut store,
                source,
                &logs.join(format!("session-{n:05}.jsonl")),
                chrono_tz::UTC,
                &PriceBook::default(),
            )?;
            assert!(report.unchanged);
            assert_eq!(report.bytes_read, 0);
        }
    }
    let unchanged_seconds = unchanged.elapsed().as_secs_f64();
    let path = logs.join("session-00000.jsonl");
    let offset = path.metadata()?.len();
    let mut append = OpenOptions::new().append(true).open(&path)?;
    usage(&mut append, 11, &timestamp)?;
    append.sync_all()?;
    let appended = path.metadata()?.len() - offset;
    let start = Instant::now();
    let resumed = collect(&mut store, "macos", &path)?;
    let append_ms = start.elapsed().as_secs_f64() * 1000.;
    assert_eq!(resumed.bytes, appended);
    assert_eq!(resumed.inserted, 1);
    assert_eq!(store.fact_count()?, 100_001);
    assert_eq!(
        store.summary(now.date_naive(), chrono_tz::UTC)?.total,
        120_001_200
    );
    Ok(
        json!({"sessions":10000,"facts":100000,"source_count":2,"fixture_bytes":bytes,
        "generation_seconds":generation_seconds,"passes":passes,
        "unchanged":{"files_checked":20000,"bytes_read":0,"seconds":unchanged_seconds},
        "append":{"bytes_read":resumed.bytes,"inserted":1,"ms":append_ms},
        "correctness":"mirror inserted zero duplicate facts; cached input counted once; unchanged content unread; append resumed at checkpoint"}),
    )
}

fn large(
    directory: &Path,
    mib: u64,
    now: DateTime<Utc>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let path = directory.join("large.jsonl");
    let timestamp = now.to_rfc3339();
    let target = mib * 1024 * 1024;
    let generate = Instant::now();
    let mut writer = BufWriter::new(File::create(&path)?);
    header(&mut writer, "benchmark-large", &timestamp)?;
    usage(&mut writer, 1, &timestamp)?;
    // A valid ignored message, below the record limit. Repeat without retaining the file body.
    let mut body = serde_json::to_vec(&json!({"type":"response_item","payload":{"type":"message",
        "role":"user","content":[{"type":"input_text","text":"x".repeat(128 * 1024)}]}}))?;
    body.push(b'\n');
    let mut bytes = 0u64;
    while bytes < target {
        writer.write_all(&body)?;
        bytes += body.len() as u64;
    }
    usage(&mut writer, 2, &timestamp)?;
    writer.flush()?;
    drop(writer);
    let fixture_bytes = path.metadata()?.len();
    let generation_seconds = generate.elapsed().as_secs_f64();
    let mut store = Store::open(directory.join("large.sqlite"))?;
    let start = Instant::now();
    let totals = collect(&mut store, "macos", &path)?;
    let seconds = start.elapsed().as_secs_f64();
    // The batch byte limit can split a normal JSON record. Its bounded tail is
    // retried from the last complete-record checkpoint on the next batch.
    assert!(totals.bytes >= fixture_bytes);
    assert_eq!(
        store
            .cursor("macos", &path.to_string_lossy())?
            .unwrap()
            .offset,
        fixture_bytes
    );
    assert_eq!(totals.inserted, 2);
    assert_eq!(store.fact_count()?, 2);
    assert_eq!(store.summary(now.date_naive(), chrono_tz::UTC)?.total, 2400);
    let unchanged = read_batch(
        &mut store,
        "macos",
        &path,
        chrono_tz::UTC,
        &PriceBook::default(),
    )?;
    assert!(unchanged.unchanged);
    assert_eq!(unchanged.bytes_read, 0);
    Ok(
        json!({"requested_mib":mib,"fixture_bytes":fixture_bytes,"generation_seconds":generation_seconds,
        "seconds":seconds,"retried_bytes":totals.bytes - fixture_bytes,"read":totals,"unchanged_bytes_read":0,
        "correctness":"valid ignored message bodies streamed; usage before and after large body collected; bounded batches; unchanged content unread"}),
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let directory = tempfile::Builder::new()
        .prefix("codexpulse-collect-bench-")
        .tempdir()?;
    let now = Utc::now();
    let result = match args.as_slice() {
        [mode] if mode == "history" => history(directory.path(), now)?,
        [mode, size] if mode == "large" => {
            let mib: u64 = size.parse()?;
            if !(1..=8192).contains(&mib) {
                return Err("large fixture size must be 1..8192 MiB".into());
            }
            large(directory.path(), mib, now)?
        }
        _ => return Err("usage: collector_benchmark history | large <MiB>".into()),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"profile":if cfg!(debug_assertions){"debug"}else{"release"},
        "target_os":std::env::consts::OS,"generated_at":now.to_rfc3339(),"result":result})
        )?
    );
    Ok(())
}
