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
        2
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
