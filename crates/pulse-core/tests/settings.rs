use pulse_core::storage::Store;
use serde_json::json;

#[test]
fn paired_settings_and_identity_cache_are_atomic_on_failure_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite");
    let mut store = Store::open(&path).unwrap();
    store
        .set_settings(&[
            ("quota", json!("old account")),
            ("app", json!("old source")),
        ])
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_app BEFORE INSERT ON settings WHEN NEW.key='app' BEGIN SELECT RAISE(ABORT,'settings failure'); END;").unwrap();
    assert!(
        store
            .set_settings(&[("quota", json!(null)), ("app", json!("new source"))])
            .is_err()
    );
    drop(store);
    let mut store = Store::open(&path).unwrap();
    assert_eq!(
        store.setting::<String>("quota").unwrap().unwrap(),
        "old account"
    );
    assert_eq!(
        store.setting::<String>("app").unwrap().unwrap(),
        "old source"
    );
    connection.execute_batch("DROP TRIGGER fail_app").unwrap();
    store
        .set_settings(&[("quota", json!(null)), ("app", json!("new source"))])
        .unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store
            .setting::<serde_json::Value>("quota")
            .unwrap()
            .unwrap(),
        json!(null)
    );
    assert_eq!(
        store.setting::<String>("app").unwrap().unwrap(),
        "new source"
    );
}
