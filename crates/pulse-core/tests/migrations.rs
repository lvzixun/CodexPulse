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
        4
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
        4
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
