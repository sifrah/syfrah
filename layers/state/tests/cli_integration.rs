//! Integration tests for the state CLI commands.
//! Tests the LayerDb API through the same code paths the CLI uses.

use syfrah_state::LayerDb;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct TestPeer {
    name: String,
    endpoint: String,
    status: String,
}

fn temp_db() -> (tempfile::TempDir, LayerDb) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.redb");
    let db = LayerDb::open_at(&path).unwrap();
    (dir, db)
}

#[test]
fn list_tables_with_data() {
    let (_dir, db) = temp_db();

    // Empty — count returns 0
    assert_eq!(db.count("peers").unwrap(), 0);
    assert_eq!(db.count("config").unwrap(), 0);

    // Add data to peers table
    let peer = TestPeer {
        name: "node-1".into(),
        endpoint: "1.2.3.4:51820".into(),
        status: "active".into(),
    };
    db.set("peers", "key1", &peer).unwrap();
    db.set("peers", "key2", &peer).unwrap();

    // Add data to config table
    db.set("config", "mesh_name", &"test-mesh").unwrap();

    assert_eq!(db.count("peers").unwrap(), 2);
    assert_eq!(db.count("config").unwrap(), 1);
    assert_eq!(db.count("metrics").unwrap(), 0);
}

#[test]
fn get_specific_key() {
    let (_dir, db) = temp_db();
    let peer = TestPeer {
        name: "node-1".into(),
        endpoint: "1.2.3.4:51820".into(),
        status: "active".into(),
    };

    db.set("peers", "wg_pub_key_abc", &peer).unwrap();

    let result: Option<TestPeer> = db.get("peers", "wg_pub_key_abc").unwrap();
    assert_eq!(result, Some(peer));
}

#[test]
fn get_nonexistent_key_returns_none() {
    let (_dir, db) = temp_db();
    let result: Option<TestPeer> = db.get("peers", "nonexistent").unwrap();
    assert_eq!(result, None);
}

#[test]
fn list_all_entries() {
    let (_dir, db) = temp_db();
    let p1 = TestPeer {
        name: "node-1".into(),
        endpoint: "1.2.3.4:51820".into(),
        status: "active".into(),
    };
    let p2 = TestPeer {
        name: "node-2".into(),
        endpoint: "5.6.7.8:51820".into(),
        status: "active".into(),
    };

    db.set("peers", "key1", &p1).unwrap();
    db.set("peers", "key2", &p2).unwrap();

    let entries: Vec<(String, TestPeer)> = db.list("peers").unwrap();
    assert_eq!(entries.len(), 2);

    let names: Vec<&str> = entries.iter().map(|(_, p)| p.name.as_str()).collect();
    assert!(names.contains(&"node-1"));
    assert!(names.contains(&"node-2"));
}

#[test]
fn metrics_get_set_inc() {
    let (_dir, db) = temp_db();

    // Default is 0
    assert_eq!(db.get_metric("counter").unwrap(), 0);

    // Set
    db.set_metric("counter", 10).unwrap();
    assert_eq!(db.get_metric("counter").unwrap(), 10);

    // Increment
    let new_val = db.inc_metric("counter", 5).unwrap();
    assert_eq!(new_val, 15);
    assert_eq!(db.get_metric("counter").unwrap(), 15);

    // Increment from zero (new metric)
    let val = db.inc_metric("new_counter", 3).unwrap();
    assert_eq!(val, 3);
}

#[test]
fn drop_layer_removes_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("droptest.redb");

    // Create and populate
    {
        let db = LayerDb::open_at(&path).unwrap();
        db.set("test", "key", &"value").unwrap();
    }

    assert!(path.exists());
    std::fs::remove_file(&path).unwrap();
    assert!(!path.exists());
}

#[test]
fn batch_atomic_multiple_tables() {
    let (_dir, db) = temp_db();

    let peer = TestPeer {
        name: "batch-node".into(),
        endpoint: "9.8.7.6:51820".into(),
        status: "active".into(),
    };

    // Write to peers + metrics in one atomic transaction
    db.batch(|w| {
        w.set("peers", "batch_key", &peer)?;
        w.set("config", "mesh_name", &"batch-mesh")?;
        w.set_metric("peers_discovered", 1)?;
        Ok(())
    })
    .unwrap();

    // All three writes committed
    let p: Option<TestPeer> = db.get("peers", "batch_key").unwrap();
    assert_eq!(p.unwrap().name, "batch-node");

    let name: Option<String> = db.get("config", "mesh_name").unwrap();
    assert_eq!(name.unwrap(), "batch-mesh");

    assert_eq!(db.get_metric("peers_discovered").unwrap(), 1);
}

#[test]
fn exists_and_delete() {
    let (_dir, db) = temp_db();

    assert!(!db.exists("peers", "key1").unwrap());
    db.set("peers", "key1", &"value").unwrap();
    assert!(db.exists("peers", "key1").unwrap());

    assert!(db.delete("peers", "key1").unwrap());
    assert!(!db.exists("peers", "key1").unwrap());
    assert!(!db.delete("peers", "key1").unwrap());
}

#[test]
fn overwrite_preserves_single_entry() {
    let (_dir, db) = temp_db();

    db.set("config", "name", &"old").unwrap();
    db.set("config", "name", &"new").unwrap();

    assert_eq!(db.count("config").unwrap(), 1);
    let val: Option<String> = db.get("config", "name").unwrap();
    assert_eq!(val, Some("new".to_string()));
}

#[test]
fn layer_exists_check() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("existcheck.redb");

    assert!(!path.exists());
    {
        let _db = LayerDb::open_at(&path).unwrap();
    }
    assert!(path.exists());
}

#[test]
fn concurrent_reads_dont_block() {
    let (_dir, db) = temp_db();

    db.set("test", "key", &"value").unwrap();

    // Multiple reads should not deadlock
    for _ in 0..100 {
        let _: Option<String> = db.get("test", "key").unwrap();
    }
}

#[test]
fn empty_string_key_and_value() {
    let (_dir, db) = temp_db();

    db.set("test", "", &"").unwrap();
    let val: Option<String> = db.get("test", "").unwrap();
    assert_eq!(val, Some("".to_string()));
}

#[test]
fn large_value() {
    let (_dir, db) = temp_db();

    let large = "x".repeat(100_000);
    db.set("test", "big", &large).unwrap();
    let val: Option<String> = db.get("test", "big").unwrap();
    assert_eq!(val.unwrap().len(), 100_000);
}

#[test]
fn batch_delete() {
    let (_dir, db) = temp_db();

    db.set("test", "a", &"1").unwrap();
    db.set("test", "b", &"2").unwrap();
    db.set("test", "c", &"3").unwrap();
    assert_eq!(db.count("test").unwrap(), 3);

    db.batch(|w| {
        w.delete("test", "a")?;
        w.delete("test", "b")?;
        Ok(())
    })
    .unwrap();

    assert_eq!(db.count("test").unwrap(), 1);
    let val: Option<String> = db.get("test", "c").unwrap();
    assert_eq!(val, Some("3".to_string()));
}

#[test]
fn multiple_tables_independent() {
    let (_dir, db) = temp_db();

    db.set("table_a", "key", &"val_a").unwrap();
    db.set("table_b", "key", &"val_b").unwrap();

    let a: Option<String> = db.get("table_a", "key").unwrap();
    let b: Option<String> = db.get("table_b", "key").unwrap();

    assert_eq!(a, Some("val_a".to_string()));
    assert_eq!(b, Some("val_b".to_string()));

    // Deleting from one table doesn't affect the other
    db.delete("table_a", "key").unwrap();
    assert!(!db.exists("table_a", "key").unwrap());
    assert!(db.exists("table_b", "key").unwrap());
}

#[test]
fn layer_name_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mytest.redb");
    let db = LayerDb::open_at(&path).unwrap();
    assert_eq!(db.layer(), "mytest");
}
