use pulse_core::storage::{Store, StoreError};
use rusqlite::Connection;

#[test]
fn reopen_does_not_rebuild_dropped_indexes_or_run_schema_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite");
    drop(Store::open(&path).unwrap());
    let read_version = || {
        Connection::open(&path)
            .unwrap()
            .query_row("PRAGMA schema_version", [], |r| r.get::<_, u32>(0))
            .unwrap()
    };
    let version = read_version();
    drop(Store::open(&path).unwrap());
    assert_eq!(read_version(), version);
}

#[test]
fn legacy_schema_upgrades_and_newer_schema_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(include_str!("../migrations/001.sql"))
        .unwrap();
    drop(connection);
    drop(Store::open(&path).unwrap());
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT MAX(version) FROM schema_version", [], |r| r
                .get::<_, u32>(0))
            .unwrap(),
        5
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE name='facts_time'",
                [],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
        0
    );
    connection
        .execute("INSERT INTO schema_version VALUES(999)", [])
        .unwrap();
    assert!(matches!(Store::open(&path), Err(StoreError::NewerSchema)));
}
#[test]
fn schema_four_adds_covering_model_index_without_changing_ledger_or_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("schema4.sqlite");
    let connection = Connection::open(&path).unwrap();
    for sql in [
        include_str!("../migrations/001.sql"),
        include_str!("../migrations/002-query-indexes.sql"),
        include_str!("../migrations/003-session-integrity.sql"),
        include_str!("../migrations/004-session-titles.sql"),
    ] {
        connection.execute_batch(sql).unwrap();
    }
    connection.execute_batch("INSERT INTO sessions VALUES('s','{}','2026-10-06T00:00:00Z');
        INSERT INTO usage_facts VALUES('f','s','m','2026-10-06T00:00:00.000Z',12,10,3,2,NULL,'{\"retained\":true}');
        INSERT INTO settings VALUES('app','{\"retained\":true}');
        INSERT INTO file_cursors VALUES('test','file',123,'identity','1',1,'{}');").unwrap();
    drop(connection);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.fact_count().unwrap(), 1);
    assert_eq!(
        store.setting::<serde_json::Value>("app").unwrap().unwrap()["retained"],
        true
    );
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT json FROM usage_facts WHERE id='f'", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "{\"retained\":true}"
    );
    assert_eq!(
        connection
            .query_row("SELECT offset FROM file_cursors", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        123
    );
    let plan = connection.prepare("EXPLAIN QUERY PLAN SELECT model,SUM(total),SUM(input),SUM(cached),SUM(output),SUM(cost_nanousd),COUNT(DISTINCT session_id) FROM usage_facts WHERE occurred_at>='2026-10-01' AND occurred_at<'2026-11-01' GROUP BY model").unwrap()
        .query_map([], |r|r.get::<_,String>(3)).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
    assert!(
        plan.iter()
            .any(|s| s.contains("COVERING INDEX facts_time_model_counts")),
        "{plan:?}"
    );
}

#[test]
fn schema_three_adds_independent_titles_without_rebuilding_usage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("schema3.sqlite");
    let connection = Connection::open(&path).unwrap();
    for sql in [
        include_str!("../migrations/001.sql"),
        include_str!("../migrations/002-query-indexes.sql"),
        include_str!("../migrations/003-session-integrity.sql"),
    ] {
        connection.execute_batch(sql).unwrap();
    }
    drop(connection);
    drop(Store::open(&path).unwrap());
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT MAX(version) FROM schema_version", [], |r| r
                .get::<_, u32>(0))
            .unwrap(),
        5
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM session_titles", [], |r| r
                .get::<_, u32>(0))
            .unwrap(),
        0
    );
}

#[test]
fn legacy_activity_migration_normalizes_metadata_in_multiple_batches() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("activity.sqlite");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(include_str!("../migrations/001.sql"))
        .unwrap();
    connection
        .execute_batch(include_str!("../migrations/002-query-indexes.sql"))
        .unwrap();
    for i in 0..260 {
        let meta = pulse_core::domain::SessionMeta {
            id: format!("s{i:03}"),
            last_activity: "2026-10-01T08:00:00+08:00".into(),
            ..Default::default()
        };
        connection
            .execute(
                "INSERT INTO sessions VALUES (?1,?2,?3)",
                rusqlite::params![
                    meta.id,
                    serde_json::to_string(&meta).unwrap(),
                    meta.last_activity
                ],
            )
            .unwrap();
    }
    drop(connection);
    let store = Store::open(&path).unwrap();
    for id in ["s000", "s127", "s128", "s259"] {
        assert_eq!(
            store.session_by_id(id).unwrap().unwrap().meta.last_activity,
            "2026-10-01T00:00:00.000000000Z"
        );
    }
    let connection = Connection::open(&path).unwrap();
    assert_eq!(connection.query_row("SELECT COUNT(*) FROM sessions WHERE last_activity='2026-10-01T00:00:00.000000000Z'",[],|r| r.get::<_,u32>(0)).unwrap(),260);
}
