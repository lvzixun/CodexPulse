use pulse_core::{
    collectors::jsonl::{MAX_RECORD_BYTES, read_batch},
    pricing::PriceBook,
    storage::Store,
};
use serde_json::json;
use std::{fs, io::Write, path::Path};

fn fixture(id: &str) -> String {
    [json!({"type":"session_meta","timestamp":"2026-10-01T00:00:00Z","payload":{"id":id,"model_provider":"openai"}}),
     json!({"type":"turn_context","payload":{"model":"test-model","turn_id":"turn"}}),
     usage(100, "2026-10-01T00:01:00Z")].into_iter().map(|r| r.to_string()+"\n").collect()
}
fn usage(n: u64, ts: &str) -> serde_json::Value {
    json!({"type":"event_msg","timestamp":ts,"payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":n,"cached_input_tokens":0,"output_tokens":10,"total_tokens":n+10}}}})
}
fn append(path: &Path, bytes: &[u8]) {
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}
fn read(store: &mut Store, path: &Path) -> pulse_core::collectors::jsonl::ReadReport {
    read_batch(
        store,
        "windows",
        path,
        chrono_tz::UTC,
        &PriceBook::default(),
    )
    .unwrap()
}
#[test]
fn activity_query_tracks_only_committed_rate_samples_without_recounting_usage() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("live.jsonl");
    let records = [
        json!({"type":"session_meta","timestamp":"2026-10-01T00:00:00Z","payload":{"id":"live"}}),
        json!({"type":"turn_context","payload":{"model":"test-model","turn_id":"turn"}}),
        json!({"type":"event_msg","timestamp":"2026-10-01T00:00:00Z","payload":{"type":"task_started","turn_id":"turn"}}),
        usage(100, "2026-10-01T00:01:00Z"),
    ];
    fs::write(
        &path,
        records
            .iter()
            .map(|r| r.to_string() + "\n")
            .collect::<String>(),
    )
    .unwrap();
    let mut store = Store::in_memory().unwrap();
    read(&mut store, &path);
    let ids = vec!["live".into(), "missing".into()];
    let first = store.session_activity(&ids).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].current_model.as_deref(), Some("test-model"));
    assert_eq!(first[0].output_rate.as_ref().unwrap().elapsed_ms, 60_000);
    let mut sample = usage(200, "2026-10-01T00:02:00Z");
    sample["payload"]["info"]["total_token_usage"]["output_tokens"] = json!(30);
    sample["payload"]["info"]["total_token_usage"]["total_tokens"] = json!(230);
    append(&path, sample.to_string().as_bytes());
    assert_eq!(read(&mut store, &path).records, 0);
    assert_eq!(store.session_activity(&ids).unwrap()[0], first[0]);
    append(&path, b"\n");
    assert_eq!(read(&mut store, &path).records, 1);
    let second = store.session_activity(&ids).unwrap();
    assert_eq!(second[0].output_rate.as_ref().unwrap().output_tokens, 30);
    assert_eq!(second[0].output_rate.as_ref().unwrap().elapsed_ms, 120_000);
    let total = store.session_by_id("live").unwrap().unwrap().total;
    for _ in 0..5 {
        assert_eq!(store.session_activity(&ids).unwrap(), second);
    }
    assert_eq!(store.session_by_id("live").unwrap().unwrap().total, total);
    assert!(store.session_activity(&[]).unwrap().is_empty());
}
#[test]
fn unchanged_files_read_zero_content_and_appends_resume() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("session.jsonl");
    fs::write(&path, fixture("append")).unwrap();
    let mut store = Store::in_memory().unwrap();
    assert_eq!(read(&mut store, &path).inserted, 1);
    let report = read(&mut store, &path);
    assert!(report.unchanged);
    assert_eq!(report.bytes_read, 0);
    append(
        &path,
        (usage(200, "2026-10-01T00:02:00Z").to_string() + "\n").as_bytes(),
    );
    assert_eq!(read(&mut store, &path).inserted, 1);
    assert_eq!(
        store
            .model_usage("2026-10-01", "2026-10-01", chrono_tz::UTC)
            .unwrap()[0]
            .total,
        210
    );
}
#[test]
fn old_checkpoint_projects_current_model_without_reading_or_recounting_logs() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("session.jsonl");
    fs::write(&path, fixture("old-model")).unwrap();
    let mut store = Store::in_memory().unwrap();
    assert_eq!(read(&mut store, &path).inserted, 1);
    let key = path.to_string_lossy();
    let mut cursor = store.cursor("windows", &key).unwrap().unwrap();
    cursor.state.session.as_mut().unwrap().current_model = None;
    let old = cursor.state.session.clone().unwrap();
    store
        .commit_batch(&cursor, &[old], &[], &[], chrono_tz::UTC)
        .unwrap();
    let report = read(&mut store, &path);
    assert_eq!(report.bytes_read, 0);
    assert_eq!(report.inserted, 0);
    assert_eq!(
        store.recent_sessions(None, 1).unwrap()[0]
            .meta
            .current_model
            .as_deref(),
        Some("test-model")
    );
    assert!(read(&mut store, &path).unchanged);
}
#[test]
fn partial_record_is_not_committed_and_survives_restart() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("session.jsonl");
    let db = temp.path().join("pulse.db");
    let head = fixture("partial");
    let next = usage(200, "2026-10-01T00:02:00Z").to_string() + "\n";
    fs::write(&path, head.as_bytes()).unwrap();
    append(&path, &next.as_bytes()[..20]);
    let mut store = Store::open(&db).unwrap();
    assert_eq!(read(&mut store, &path).inserted, 1);
    assert_eq!(
        store
            .cursor("windows", &path.to_string_lossy())
            .unwrap()
            .unwrap()
            .offset,
        head.len() as u64
    );
    drop(store);
    append(&path, &next.as_bytes()[20..]);
    let mut store = Store::open(&db).unwrap();
    assert_eq!(read(&mut store, &path).inserted, 1);
    assert_eq!(store.fact_count().unwrap(), 2);
}
#[test]
fn oversized_record_is_bounded_and_following_usage_is_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("session.jsonl");
    fs::write(&path, fixture("huge")).unwrap();
    append(&path, &vec![b'x'; MAX_RECORD_BYTES * 40]);
    append(&path, b"\n");
    append(
        &path,
        (usage(200, "2026-10-01T00:02:00Z").to_string() + "\n").as_bytes(),
    );
    let mut store = Store::in_memory().unwrap();
    let mut issues = 0;
    loop {
        let report = read(&mut store, &path);
        assert!(report.bytes_read <= 8 * 1024 * 1024);
        issues += report.issues;
        if !report.more {
            break;
        }
    }
    assert_eq!(issues, 1);
    assert_eq!(store.fact_count().unwrap(), 2);
    assert_eq!(read(&mut store, &path).bytes_read, 0);
}
#[test]
fn archive_copy_is_read_without_double_counting() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("session.jsonl");
    let archive = temp.path().join("archive.jsonl");
    fs::write(&path, fixture("same")).unwrap();
    fs::copy(&path, &archive).unwrap();
    let mut store = Store::in_memory().unwrap();
    assert_eq!(read(&mut store, &path).inserted, 1);
    assert_eq!(read(&mut store, &archive).inserted, 0);
}
#[test]
fn truncation_restarts_parser_without_losing_previous_usage() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("session.jsonl");
    fs::write(&path, fixture("previous-long-identifier")).unwrap();
    let mut store = Store::in_memory().unwrap();
    read(&mut store, &path);
    fs::write(&path, fixture("new")).unwrap();
    assert_eq!(read(&mut store, &path).inserted, 1);
    assert_eq!(store.fact_count().unwrap(), 2);
}
