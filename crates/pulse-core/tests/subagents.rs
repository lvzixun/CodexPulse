use chrono_tz::UTC;
use pulse_core::{
    collectors::jsonl::read_batch,
    domain::{SessionMeta, TokenCounts, UsageFact},
    ledger::ParserState,
    pricing::PriceBook,
    storage::{FileCursor, ModelPageRequest, PageDirection, SessionPageRequest, Store},
};
use serde_json::json;

#[test]
fn subagents_are_hidden_before_pagination_and_counts_but_keep_usage() {
    let mut store = Store::in_memory().unwrap();
    let mut sessions = Vec::new();
    let mut facts = Vec::new();
    for i in 0..46 {
        let id = format!("session-{i:02}");
        let child = i >= 16;
        sessions.push(SessionMeta {
            id: id.clone(),
            source_kind: Some(if child { "subagent" } else { "cli" }.into()),
            parent_id: (i == 0).then(|| "ordinary-fork-parent".into()),
            last_activity: "2026-10-06T08:59:00Z".into(),
            status: "active".into(),
            ..Default::default()
        });
        facts.push(UsageFact {
            id: format!("fact-{i}"),
            session_id: id,
            turn_id: None,
            model: "test-model".into(),
            provider: "test".into(),
            timestamp: "2026-10-06T08:59:00Z".into(),
            tokens: TokenCounts {
                input: Some(100),
                output: Some(20),
                total: Some(120),
                ..Default::default()
            },
            service_tier: None,
            request_input: None,
            quality: "test".into(),
            price_version: None,
            cost_nanousd: Some(3),
        });
    }
    let cursor = FileCursor {
        source_id: "test".into(),
        file_key: "test".into(),
        offset: 1,
        file_identity: "test".into(),
        modified_ns: "1".into(),
        parser_version: pulse_core::PARSER_VERSION,
        state: ParserState::default(),
    };
    store
        .commit_batch(&cursor, &sessions, &facts, &[], UTC)
        .unwrap();
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-06T09:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let first = store
        .session_page_with_activity(&Default::default(), UTC, now, &["test".into()])
        .unwrap();
    assert_eq!(first.items.len(), 10);
    assert!(
        first
            .items
            .iter()
            .all(|s| s.meta.source_kind.as_deref() == Some("cli"))
    );
    let second = store
        .session_page_with_activity(
            &SessionPageRequest {
                cursor: first.older,
                ..Default::default()
            },
            UTC,
            now,
            &["test".into()],
        )
        .unwrap();
    assert_eq!(second.items.len(), 6);
    assert!(second.older.is_none());
    assert!(second.items.iter().any(|s| s.meta.parent_id.is_some())); // Ordinary forks stay visible.
    let back = store
        .session_page_with_activity(
            &SessionPageRequest {
                cursor: second.newer,
                direction: PageDirection::Newer,
                ..Default::default()
            },
            UTC,
            now,
            &["test".into()],
        )
        .unwrap();
    assert_eq!(back.items.len(), 10);
    assert_eq!(store.recent_sessions(None, 100).unwrap().len(), 16);
    assert_eq!(
        store
            .session_page(&Default::default(), UTC)
            .unwrap()
            .items
            .len(),
        10
    );
    let summary = store.summary(now.date_naive(), UTC).unwrap();
    assert_eq!(summary.sessions, 16);
    assert_eq!(summary.total, 46 * 120);
    assert_eq!(summary.cost_nanousd, 46 * 3);
    assert_eq!(store.fact_count().unwrap(), 46);
    let models = store.model_usage("2026-10-06", "2026-10-06", UTC).unwrap();
    assert_eq!(models[0].sessions, 16);
    assert_eq!(models[0].total, 46 * 120);
    let page = store
        .model_page(
            &ModelPageRequest {
                from_day: "2026-10-06".into(),
                through_day: "2026-10-06".into(),
                ..Default::default()
            },
            UTC,
        )
        .unwrap();
    assert_eq!(page.items[0].usage.sessions, 16);
    assert_eq!(page.items[0].usage.cost_nanousd, 46 * 3);
    let scoped = store
        .session_page(
            &SessionPageRequest {
                filter: pulse_core::storage::SessionFilter {
                    model: Some("test-model".into()),
                    from_day: Some("2026-10-06".into()),
                    through_day: Some("2026-10-06".into()),
                },
                ..Default::default()
            },
            UTC,
        )
        .unwrap();
    assert_eq!(scoped.items.len(), 10);
    assert!(
        scoped
            .items
            .iter()
            .all(|s| s.meta.source_kind.as_deref() != Some("subagent"))
    );
}

#[test]
fn old_checkpoint_recovers_source_from_header_without_replaying_usage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let db = dir.path().join("ledger.sqlite");
    let header = json!({"type":"session_meta","timestamp":"2026-10-06T00:00:00Z","payload":{"id":"child","originator":"codex-tui","source":{"subagent":{"thread_spawn":{"parent_thread_id":"root"}}}}}).to_string() + "\n";
    let log = header.clone() + &(json!({"type":"turn_context","payload":{"model":"test-model","turn_id":"turn"}}).to_string()+"\n") + &(json!({"type":"event_msg","timestamp":"2026-10-06T08:00:00Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"output_tokens":20,"total_tokens":120}}}}).to_string()+"\n");
    std::fs::write(&path, &log).unwrap();
    let mut store = Store::open(&db).unwrap();
    assert_eq!(
        read_batch(&mut store, "test", &path, UTC, &PriceBook::default())
            .unwrap()
            .inserted,
        1
    );
    let key = path.to_string_lossy();
    let before = store.cursor("test", &key).unwrap().unwrap();
    let original = store.session_by_id("child").unwrap().unwrap();
    assert_eq!(original.meta.source_kind.as_deref(), Some("subagent"));
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection.execute("UPDATE file_cursors SET parser_state=json_set(json_remove(parser_state,'$.session_source_known'),'$.session.source_kind','codex-tui')", []).unwrap();
    connection
        .execute(
            "UPDATE sessions SET metadata=json_set(metadata,'$.source_kind','codex-tui')",
            [],
        )
        .unwrap();
    let report = read_batch(&mut store, "test", &path, UTC, &PriceBook::default()).unwrap();
    assert!(!report.unchanged);
    assert_eq!(report.bytes_read, header.len());
    assert_eq!((report.records, report.inserted), (0, 0));
    let after = store.cursor("test", &key).unwrap().unwrap();
    assert_eq!(after.offset, before.offset);
    assert_eq!(after.state.cumulative, before.state.cumulative);
    assert_eq!(after.state.ordinal, before.state.ordinal);
    assert!(after.state.session_source_known);
    let session = store.session_by_id("child").unwrap().unwrap();
    assert_eq!(session.meta.source_kind.as_deref(), Some("subagent"));
    assert_eq!(session.meta.last_activity, original.meta.last_activity);
    assert_eq!(session.meta.output_rate, original.meta.output_rate);
    assert_eq!(session.total, original.total);
    assert_eq!(store.fact_count().unwrap(), 1);
    assert!(store.recent_sessions(None, 10).unwrap().is_empty());
    drop(store);
    let mut store = Store::open(&db).unwrap();
    let unchanged = read_batch(&mut store, "test", &path, UTC, &PriceBook::default()).unwrap();
    assert!(unchanged.unchanged);
    assert_eq!(unchanged.bytes_read, 0);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), log);
    let tail = json!({"type":"event_msg","timestamp":"2026-10-06T08:01:00Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":200,"output_tokens":40,"total_tokens":240}}}}).to_string() + "\n";
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(tail.as_bytes())
        .unwrap();
    assert_eq!(
        read_batch(&mut store, "test", &path, UTC, &PriceBook::default())
            .unwrap()
            .inserted,
        1
    );
    assert_eq!(store.session_by_id("child").unwrap().unwrap().total, 240);
    assert_eq!(store.fact_count().unwrap(), 2);
    assert!(store.recent_sessions(None, 10).unwrap().is_empty());
}
