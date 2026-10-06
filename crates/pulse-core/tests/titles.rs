use chrono_tz::UTC;
use pulse_core::{
    collectors::jsonl::{read_batch, read_titles},
    domain::SessionMeta,
    ledger::ParserState,
    pricing::PriceBook,
    storage::{FileCursor, SessionDetailRequest, SessionPageRequest, SessionTitle, Store},
};
use std::{fs, io::Write};

fn row(id: &str, name: &str, at: &str) -> String {
    serde_json::json!({"id":id,"thread_name":name,"updated_at":at}).to_string() + "\n"
}
fn cursor(source: &str, offset: u64) -> FileCursor {
    FileCursor {
        source_id: source.into(),
        file_key: "index".into(),
        offset,
        file_identity: "test".into(),
        modified_ns: "1".into(),
        parser_version: pulse_core::PARSER_VERSION,
        state: ParserState::default(),
    }
}
fn metadata(store: &mut Store, fallback: &str) {
    let meta = SessionMeta {
        id: "thread".into(),
        title: Some(fallback.into()),
        project: Some("ni-x20".into()),
        last_activity: "2026-10-06T00:00:00Z".into(),
        ..Default::default()
    };
    store
        .commit_batch(&cursor("rollout", 1), &[meta], &[], &[], UTC)
        .unwrap();
}
fn title(store: &Store) -> Option<String> {
    store.session_by_id("thread").unwrap().unwrap().meta.title
}
fn update(store: &mut Store, source: &str, name: &str, at: &str, offset: u64) -> usize {
    store
        .commit_titles(
            &cursor(source, offset),
            &[SessionTitle::parse(row("thread", name, at).as_bytes()).unwrap()],
            &[],
        )
        .unwrap()
}

#[test]
fn index_before_rollouts_and_later_usage_cannot_overwrite_renamed_title() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session_index.jsonl");
    // Out-of-order records and UTC offsets must compare by their actual instants.
    let bytes = row("thread", "旧标题", "2026-10-06T04:43:19Z")
        + &row(
            "thread",
            "codexpulse开发",
            "2026-10-06T13:12:30.5221783+08:00",
        )
        + &row("thread", "较旧的镜像", "2026-10-06T05:12:30.5Z");
    fs::write(&path, &bytes).unwrap();
    let mut store = Store::in_memory().unwrap();
    // An older app may already have consumed this file as a generic JSONL.
    read_batch(&mut store, "windows", &path, UTC, &PriceBook::default()).unwrap();
    assert_eq!(
        read_titles(&mut store, "windows", &path).unwrap().inserted,
        2
    );
    assert!(store.recent_sessions(None, 10).unwrap().is_empty());
    metadata(&mut store, "rollout-name");
    metadata(&mut store, "name-from-later-usage");
    assert_eq!(title(&store).as_deref(), Some("codexpulse开发"));
    assert_eq!(
        store.recent_sessions(None, 10).unwrap()[0]
            .meta
            .title
            .as_deref(),
        Some("codexpulse开发")
    );
    assert_eq!(
        store
            .session_page(&SessionPageRequest::default(), UTC)
            .unwrap()
            .items[0]
            .meta
            .title
            .as_deref(),
        Some("codexpulse开发")
    );
    let detail = store
        .session_detail(&SessionDetailRequest {
            id: "thread".into(),
            models_after: None,
            prices_after: None,
        })
        .unwrap()
        .unwrap();
    assert_eq!(detail.session.meta.title.as_deref(), Some("codexpulse开发"));
    assert_eq!(detail.usage.events, 0); // Titles never create usage facts.
    assert_eq!(fs::read(&path).unwrap(), bytes.as_bytes()); // Source is read-only.
    let unchanged = read_titles(&mut store, "windows", &path).unwrap();
    assert!(unchanged.unchanged);
    assert_eq!(unchanged.bytes_read, 0);
}

#[test]
fn partial_rename_survives_restart_then_applies_after_append_and_clear() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session_index.jsonl");
    let db = dir.path().join("ledger.sqlite");
    let first = row("thread", "初始标题", "2026-10-06T00:00:00Z");
    let renamed = row("thread", "重命名后的会话", "2026-10-06T01:00:00Z");
    fs::write(&path, first.clone() + &renamed[..renamed.len() - 2]).unwrap();
    let mut store = Store::open(&db).unwrap();
    metadata(&mut store, "rollout");
    let report = read_titles(&mut store, "windows", &path).unwrap();
    assert_eq!(report.records, 1);
    assert_eq!(title(&store).as_deref(), Some("初始标题"));
    let key = format!("session-index:{}", path.to_string_lossy());
    assert_eq!(
        store.cursor("windows", &key).unwrap().unwrap().offset,
        first.len() as u64
    );
    drop(store);
    let mut store = Store::open(&db).unwrap();
    assert!(read_titles(&mut store, "windows", &path).unwrap().unchanged);
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&renamed.as_bytes()[renamed.len() - 2..])
        .unwrap();
    assert_eq!(
        read_titles(&mut store, "windows", &path).unwrap().records,
        1
    );
    assert_eq!(title(&store).as_deref(), Some("重命名后的会话"));
    // A rewritten/compacted index is re-read; empty names explicitly clear old titles.
    fs::write(&path, row("thread", " ", "2026-10-06T02:00:00Z")).unwrap();
    read_titles(&mut store, "windows", &path).unwrap();
    metadata(&mut store, "stale rollout title");
    assert_eq!(title(&store), None);
}

#[test]
fn mirrors_respect_newer_names_and_windows_equal_timestamp_ownership() {
    let mut store = Store::in_memory().unwrap();
    metadata(&mut store, "rollout");
    let at = "2026-10-06T02:00:00Z";
    update(&mut store, "wsl:Ubuntu", "新标题", at, 1);
    update(&mut store, "windows", "新标题", at, 1);
    assert_eq!(update(&mut store, "wsl:Ubuntu", "冲突镜像", at, 2), 0);
    assert_eq!(
        update(&mut store, "windows", "更早标题", "2026-10-06T01:00:00Z", 2),
        0
    );
    assert_eq!(title(&store).as_deref(), Some("新标题"));
    update(
        &mut store,
        "wsl:Ubuntu",
        "更新后的 WSL 标题",
        "2026-10-06T03:00:00Z",
        3,
    );
    assert_eq!(title(&store).as_deref(), Some("更新后的 WSL 标题"));
    let invalid =
        SessionTitle::parse(row("thread", "不能提交", "2026-10-06T04:00:00Z").as_bytes()).unwrap();
    assert!(
        store
            .commit_titles(
                &cursor("windows", u64::MAX),
                &[invalid],
                &["test_error".into()]
            )
            .is_err()
    );
    assert_eq!(title(&store).as_deref(), Some("更新后的 WSL 标题"));
    assert_eq!(store.cursor("windows", "index").unwrap().unwrap().offset, 2);
}

#[test]
fn title_index_skips_invalid_records_and_uses_bounded_batches() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session_index.jsonl");
    let mut bytes = "{bad}\n".to_owned()
        + &row("thread", "bad time", "invalid")
        + &row("", "bad id", "2026-10-06T00:00:00Z");
    for i in 0..260 {
        bytes += &row(&format!("thread-{i}"), "独立名称", "2026-10-06T00:00:00Z");
    }
    fs::write(&path, bytes).unwrap();
    let mut store = Store::in_memory().unwrap();
    let first = read_titles(&mut store, "windows", &path).unwrap();
    assert!(first.more);
    assert!(first.records <= 256);
    let mut inserts = first.inserted;
    let mut issues = first.issues;
    loop {
        let next = read_titles(&mut store, "windows", &path).unwrap();
        inserts += next.inserted;
        issues += next.issues;
        if !next.more {
            break;
        }
    }
    assert_eq!(inserts, 260);
    assert_eq!(issues, 3);
    assert!(store.recent_sessions(None, 10).unwrap().is_empty());
    assert!(
        SessionTitle::parse(row(&"x".repeat(257), "title", "2026-10-06T00:00:00Z").as_bytes())
            .is_err()
    );
}

#[test]
fn failed_checkpoint_rolls_back_titles_and_issues_together() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite");
    let mut store = Store::open(&path).unwrap();
    metadata(&mut store, "rollout");
    update(&mut store, "windows", "原标题", "2026-10-06T00:00:00Z", 1);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_checkpoint BEFORE INSERT ON file_cursors BEGIN SELECT RAISE(ABORT,'checkpoint failure'); END;").unwrap();
    let renamed =
        SessionTitle::parse(row("thread", "不能提交", "2026-10-06T01:00:00Z").as_bytes()).unwrap();
    assert!(
        store
            .commit_titles(&cursor("windows", 2), &[renamed], &["test_error".into()])
            .is_err()
    );
    assert_eq!(title(&store).as_deref(), Some("原标题"));
    assert_eq!(store.cursor("windows", "index").unwrap().unwrap().offset, 1);
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM parse_issues", [], |r| r
                .get::<_, u32>(0))
            .unwrap(),
        0
    );
}
