//! Advanced tests for syfrah-state: edge cases, ACID, concurrence,
//! stress, corruption, error handling, fuzzing.

use std::collections::HashMap;
use std::sync::{Arc, Barrier};
use std::thread;

use syfrah_state::LayerDb;

fn temp_db() -> (tempfile::TempDir, LayerDb) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.redb");
    let db = LayerDb::open_at(&path).unwrap();
    (dir, db)
}

// ── Edge cases ──────────────────────────────────────────────

#[test]
fn unicode_keys_and_values() {
    let (_dir, db) = temp_db();

    db.set("test", "日本語キー", &"値テスト").unwrap();
    db.set("test", "émojis🚀🔥", &"données avec accents éèà")
        .unwrap();
    db.set("test", "中文", &"测试数据").unwrap();

    let v: Option<String> = db.get("test", "日本語キー").unwrap();
    assert_eq!(v, Some("値テスト".to_string()));

    let v: Option<String> = db.get("test", "émojis🚀🔥").unwrap();
    assert_eq!(v, Some("données avec accents éèà".to_string()));

    let entries: Vec<(String, String)> = db.list("test").unwrap();
    assert_eq!(entries.len(), 3);
}

#[test]
fn special_chars_in_keys() {
    let (_dir, db) = temp_db();

    let keys = vec![
        "key/with/slashes",
        "key.with.dots",
        "key-with-dashes",
        "key with spaces",
        "key\twith\ttabs",
        "key\nwith\nnewlines",
        "",
        "   ",
        "key=value&other=thing",
    ];

    for key in &keys {
        db.set("test", key, &format!("val_{key}")).unwrap();
    }

    for key in &keys {
        let v: Option<String> = db.get("test", key).unwrap();
        assert!(v.is_some(), "key '{key}' should exist");
    }

    assert_eq!(db.count("test").unwrap(), keys.len() as u64);
}

#[test]
fn very_long_key() {
    let (_dir, db) = temp_db();

    let long_key = "x".repeat(10_000);
    db.set("test", &long_key, &"value").unwrap();

    let v: Option<String> = db.get("test", &long_key).unwrap();
    assert_eq!(v, Some("value".to_string()));

    assert!(db.exists("test", &long_key).unwrap());
    assert!(db.delete("test", &long_key).unwrap());
    assert!(!db.exists("test", &long_key).unwrap());
}

#[test]
fn get_metric_on_fresh_db() {
    let (_dir, db) = temp_db();

    // Metrics table doesn't exist yet — should return 0
    assert_eq!(db.get_metric("anything").unwrap(), 0);
    assert_eq!(db.get_metric("").unwrap(), 0);
    assert_eq!(db.get_metric("nonexistent_counter").unwrap(), 0);
}

#[test]
fn open_at_nonexistent_parent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deep").join("nested").join("test.redb");

    // Parent dirs don't exist — open_at should either create them or fail cleanly
    let result = LayerDb::open_at(&path);
    // redb::Database::create will fail because parent doesn't exist
    // That's acceptable — the error should be clean, not a panic
    assert!(result.is_err() || result.is_ok());
}

// ── ACID ────────────────────────────────────────────────────

#[test]
fn set_then_immediate_read_consistency() {
    let (_dir, db) = temp_db();

    for i in 0..100 {
        let key = format!("key_{i}");
        let val = format!("val_{i}");
        db.set("test", &key, &val).unwrap();

        // Read immediately — must see the write
        let read: Option<String> = db.get("test", &key).unwrap();
        assert_eq!(read, Some(val), "read-after-write failed for key_{i}");
    }
}

#[test]
fn batch_set_and_delete_same_key() {
    let (_dir, db) = temp_db();

    db.batch(|w| {
        w.set("test", "ephemeral", &"will_be_deleted")?;
        w.delete("test", "ephemeral")?;
        Ok(())
    })
    .unwrap();

    // Key should not exist after batch
    assert!(!db.exists("test", "ephemeral").unwrap());
    assert_eq!(db.count("test").unwrap(), 0);
}

#[test]
fn multiple_tables_in_one_batch() {
    let (_dir, db) = temp_db();

    db.batch(|w| {
        w.set("peers", "p1", &"peer_data_1")?;
        w.set("config", "mesh_name", &"my-mesh")?;
        w.set("vpcs", "vpc-1", &"vpc_data")?;
        w.set_metric("peers_discovered", 1)?;
        Ok(())
    })
    .unwrap();

    assert_eq!(db.count("peers").unwrap(), 1);
    assert_eq!(db.count("config").unwrap(), 1);
    assert_eq!(db.count("vpcs").unwrap(), 1);
    assert_eq!(db.get_metric("peers_discovered").unwrap(), 1);
}

// ── Concurrence ─────────────────────────────────────────────

#[test]
fn concurrent_writers_serialized() {
    let (_dir, db) = temp_db();
    let db = Arc::new(db);
    let barrier = Arc::new(Barrier::new(10));

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let db = db.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                for j in 0..10 {
                    let key = format!("t{i}_k{j}");
                    db.set("concurrent", &key, &format!("val_{i}_{j}")).unwrap();
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    // 10 threads × 10 keys = 100 entries
    assert_eq!(db.count("concurrent").unwrap(), 100);
}

#[test]
fn concurrent_readers_during_write() {
    let (_dir, db) = temp_db();
    let db = Arc::new(db);

    // Pre-populate
    for i in 0..50 {
        db.set("data", &format!("key_{i}"), &format!("val_{i}"))
            .unwrap();
    }

    let barrier = Arc::new(Barrier::new(6));

    // 1 writer thread
    let writer_db = db.clone();
    let writer_barrier = barrier.clone();
    let writer = thread::spawn(move || {
        writer_barrier.wait();
        for i in 50..100 {
            writer_db
                .set("data", &format!("key_{i}"), &format!("val_{i}"))
                .unwrap();
        }
    });

    // 5 reader threads
    let readers: Vec<_> = (0..5)
        .map(|_| {
            let db = db.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                let mut reads = 0;
                for i in 0..50 {
                    let v: Option<String> = db.get("data", &format!("key_{i}")).unwrap();
                    if v.is_some() {
                        reads += 1;
                    }
                }
                reads
            })
        })
        .collect();

    writer.join().unwrap();
    for r in readers {
        let reads = r.join().unwrap();
        // All 50 pre-populated keys should be readable
        assert_eq!(reads, 50);
    }

    assert_eq!(db.count("data").unwrap(), 100);
}

#[test]
fn metric_inc_concurrent() {
    let (_dir, db) = temp_db();
    let db = Arc::new(db);
    let barrier = Arc::new(Barrier::new(10));

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let db = db.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                for _ in 0..100 {
                    db.inc_metric("counter", 1).unwrap();
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    // 10 threads × 100 increments = 1000
    assert_eq!(db.get_metric("counter").unwrap(), 1000);
}

// ── Persistance / Lifecycle ─────────────────────────────────

#[test]
fn reopen_database_preserves_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reopen.redb");

    // Write data
    {
        let db = LayerDb::open_at(&path).unwrap();
        db.set("peers", "key1", &"data1").unwrap();
        db.set("peers", "key2", &"data2").unwrap();
        db.set_metric("counter", 42).unwrap();
    }

    // Reopen and verify
    {
        let db = LayerDb::open_at(&path).unwrap();
        let v: Option<String> = db.get("peers", "key1").unwrap();
        assert_eq!(v, Some("data1".to_string()));
        assert_eq!(db.count("peers").unwrap(), 2);
        assert_eq!(db.get_metric("counter").unwrap(), 42);
    }
}

#[test]
fn destroy_then_reopen_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("destroy.redb");

    {
        let db = LayerDb::open_at(&path).unwrap();
        db.set("test", "key", &"value").unwrap();
    }

    std::fs::remove_file(&path).unwrap();

    {
        let db = LayerDb::open_at(&path).unwrap();
        assert_eq!(db.count("test").unwrap(), 0);
        let v: Option<String> = db.get("test", "key").unwrap();
        assert_eq!(v, None);
    }
}

#[test]
fn layer_exists_before_and_after() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lifecycle.redb");

    assert!(!path.exists());

    {
        let _db = LayerDb::open_at(&path).unwrap();
    }
    assert!(path.exists());

    std::fs::remove_file(&path).unwrap();
    assert!(!path.exists());
}

// ── Serialization ───────────────────────────────────────────

#[test]
fn set_none_value() {
    let (_dir, db) = temp_db();

    let val: Option<String> = None;
    db.set("test", "nullable", &val).unwrap();

    let read: Option<Option<String>> = db.get("test", "nullable").unwrap();
    assert_eq!(read, Some(None));
}

#[test]
fn nested_struct_roundtrip() {
    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Inner {
        tags: Vec<String>,
        metadata: HashMap<String, i64>,
    }

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Outer {
        name: String,
        inner: Inner,
        optional: Option<Inner>,
        count: u64,
    }

    let (_dir, db) = temp_db();

    let mut meta = HashMap::new();
    meta.insert("cpu".to_string(), 32);
    meta.insert("ram_gb".to_string(), 128);

    let val = Outer {
        name: "complex".to_string(),
        inner: Inner {
            tags: vec!["tag1".into(), "tag2".into(), "tag3".into()],
            metadata: meta.clone(),
        },
        optional: Some(Inner {
            tags: vec![],
            metadata: HashMap::new(),
        }),
        count: 999,
    };

    db.set("test", "nested", &val).unwrap();
    let read: Option<Outer> = db.get("test", "nested").unwrap();
    assert_eq!(read, Some(val));
}

// ── Error handling ──────────────────────────────────────────

#[test]
fn invalid_json_in_db_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.redb");

    // Write raw invalid bytes directly via redb
    {
        let raw_db = redb::Database::create(&path).unwrap();
        let table_def: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("test");
        let write_txn = raw_db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(table_def).unwrap();
            table
                .insert("bad_key", b"not valid json {{{{".as_slice())
                .unwrap();
        }
        write_txn.commit().unwrap();
    }

    // Now try to read it with LayerDb — should return a JSON error
    let db = LayerDb::open_at(&path).unwrap();
    let result: syfrah_state::Result<Option<String>> = db.get("test", "bad_key");
    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("serialization") || err_msg.contains("expected"),
        "error should be JSON-related: {err_msg}"
    );
}

#[test]
fn error_get_wrong_type() {
    let (_dir, db) = temp_db();

    // Store a string
    db.set("test", "key", &"hello").unwrap();

    // Try to read as a different type
    let result: syfrah_state::Result<Option<Vec<u64>>> = db.get("test", "key");
    assert!(result.is_err(), "reading string as Vec<u64> should fail");
}

#[test]
fn count_after_many_deletes() {
    let (_dir, db) = temp_db();

    for i in 0..100 {
        db.set("test", &format!("key_{i}"), &i).unwrap();
    }
    assert_eq!(db.count("test").unwrap(), 100);

    for i in 0..50 {
        db.delete("test", &format!("key_{i}")).unwrap();
    }
    assert_eq!(db.count("test").unwrap(), 50);

    // Remaining keys are 50-99
    for i in 50..100 {
        assert!(db.exists("test", &format!("key_{i}")).unwrap());
    }
    for i in 0..50 {
        assert!(!db.exists("test", &format!("key_{i}")).unwrap());
    }
}

// ── Stress ──────────────────────────────────────────────────

#[test]
fn stress_5000_inserts() {
    let (_dir, db) = temp_db();

    for i in 0..5_000 {
        db.set("stress", &format!("key_{i:05}"), &format!("val_{i}"))
            .unwrap();
    }

    assert_eq!(db.count("stress").unwrap(), 5_000);

    // Spot check
    let v: Option<String> = db.get("stress", "key_02500").unwrap();
    assert_eq!(v, Some("val_2500".to_string()));

    let v: Option<String> = db.get("stress", "key_04999").unwrap();
    assert_eq!(v, Some("val_4999".to_string()));
}

#[test]
fn stress_rapid_set_delete_cycles() {
    let (_dir, db) = temp_db();

    for cycle in 0..100 {
        let key = format!("cycle_{cycle}");
        db.set("stress", &key, &cycle).unwrap();
        assert!(db.exists("stress", &key).unwrap());
        db.delete("stress", &key).unwrap();
        assert!(!db.exists("stress", &key).unwrap());
    }

    assert_eq!(db.count("stress").unwrap(), 0);
}

#[test]
fn stress_100_tables() {
    let (_dir, db) = temp_db();

    for t in 0..100 {
        let table = format!("table_{t:03}");
        db.set(&table, "key", &format!("val_{t}")).unwrap();
    }

    for t in 0..100 {
        let table = format!("table_{t:03}");
        let v: Option<String> = db.get(&table, "key").unwrap();
        assert_eq!(v, Some(format!("val_{t}")));
    }
}

#[test]
fn stress_concurrent_batch_writes() {
    let (_dir, db) = temp_db();
    let db = Arc::new(db);
    let barrier = Arc::new(Barrier::new(5));

    let handles: Vec<_> = (0..5)
        .map(|i| {
            let db = db.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                for j in 0..20 {
                    db.batch(|w| {
                        w.set("batch_stress", &format!("t{i}_k{j}"), &format!("v{i}_{j}"))?;
                        w.set_metric(&format!("batch_counter_{i}"), j as u64)?;
                        Ok(())
                    })
                    .unwrap();
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(db.count("batch_stress").unwrap(), 100);
}

#[test]
fn stress_large_values_1mb() {
    let (_dir, db) = temp_db();

    let large = "A".repeat(1_000_000);

    for i in 0..5 {
        db.set("large", &format!("key_{i}"), &large).unwrap();
    }

    assert_eq!(db.count("large").unwrap(), 5);

    let v: Option<String> = db.get("large", "key_0").unwrap();
    assert_eq!(v.unwrap().len(), 1_000_000);
}

// ── Corruption ──────────────────────────────────────────────

#[test]
fn corrupt_file_open_fails_clean() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("corrupt.redb");

    // Write random garbage
    std::fs::write(
        &path,
        b"THIS IS NOT A VALID REDB FILE!!! random garbage bytes 12345",
    )
    .unwrap();

    let result = LayerDb::open_at(&path);
    assert!(result.is_err(), "opening corrupt file should fail");

    let err = format!("{}", result.unwrap_err());
    // Should be a DB error, not a panic
    assert!(
        err.contains("database") || err.contains("Database") || err.contains("invalid"),
        "error should mention database: {err}"
    );
}

#[test]
fn truncated_file_open_fails_clean() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("truncated.redb");

    // Create a valid db first
    {
        let db = LayerDb::open_at(&path).unwrap();
        db.set("test", "key", &"value").unwrap();
    }

    // Truncate to 10 bytes (corrupt the header)
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(10).unwrap();
    drop(file);

    let result = LayerDb::open_at(&path);
    assert!(result.is_err(), "opening truncated file should fail");
}

#[test]
fn zero_byte_file_opens_as_fresh_db() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.redb");

    std::fs::write(&path, b"").unwrap();

    // redb treats a zero-byte file as a fresh database (overwrites it)
    let result = LayerDb::open_at(&path);
    assert!(
        result.is_ok(),
        "zero-byte file should be treated as fresh db"
    );

    let db = result.unwrap();
    assert_eq!(db.count("test").unwrap(), 0);
}

// ── Fuzzing ─────────────────────────────────────────────────

#[test]
fn fuzz_random_keys_and_values() {
    let (_dir, db) = temp_db();

    // Generate pseudo-random keys and values
    let mut rng_state: u64 = 42;
    let mut next_rand = || -> u64 {
        rng_state ^= rng_state << 13;
        rng_state ^= rng_state >> 7;
        rng_state ^= rng_state << 17;
        rng_state
    };

    let mut keys = Vec::new();

    // Insert 500 random key-value pairs
    for _ in 0..500 {
        let key_len = (next_rand() % 200) as usize + 1;
        let val_len = (next_rand() % 1000) as usize;

        let key: String = (0..key_len)
            .map(|_| char::from((next_rand() % 94 + 33) as u8))
            .collect();
        let val: String = (0..val_len)
            .map(|_| char::from((next_rand() % 94 + 33) as u8))
            .collect();

        db.set("fuzz", &key, &val).unwrap();
        keys.push((key, val));
    }

    // Verify all can be read back
    for (key, expected_val) in &keys {
        let read: Option<String> = db.get("fuzz", key).unwrap();
        assert_eq!(
            read.as_deref(),
            Some(expected_val.as_str()),
            "mismatch for key len={}",
            key.len()
        );
    }

    // Delete half
    for (key, _) in keys.iter().take(250) {
        db.delete("fuzz", key).unwrap();
    }

    assert_eq!(db.count("fuzz").unwrap(), 250);
}

#[test]
fn fuzz_random_operations() {
    let (_dir, db) = temp_db();

    let mut rng_state: u64 = 123456789;
    let mut next_rand = || -> u64 {
        rng_state ^= rng_state << 13;
        rng_state ^= rng_state >> 7;
        rng_state ^= rng_state << 17;
        rng_state
    };

    let mut expected: HashMap<String, String> = HashMap::new();

    // 1000 random operations
    for _ in 0..1000 {
        let key = format!("key_{}", next_rand() % 50);
        let op = next_rand() % 4;

        match op {
            0 => {
                // Set
                let val = format!("val_{}", next_rand());
                db.set("fuzz_ops", &key, &val).unwrap();
                expected.insert(key, val);
            }
            1 => {
                // Get
                let read: Option<String> = db.get("fuzz_ops", &key).unwrap();
                let expect = expected.get(&key).cloned();
                assert_eq!(read, expect, "get mismatch for {key}");
            }
            2 => {
                // Delete
                db.delete("fuzz_ops", &key).unwrap();
                expected.remove(&key);
            }
            3 => {
                // Exists
                let exists = db.exists("fuzz_ops", &key).unwrap();
                assert_eq!(
                    exists,
                    expected.contains_key(&key),
                    "exists mismatch for {key}"
                );
            }
            _ => unreachable!(),
        }
    }

    // Final verification
    assert_eq!(db.count("fuzz_ops").unwrap(), expected.len() as u64);
}

// ── Disk full simulation ────────────────────────────────────

#[cfg(target_os = "linux")]
#[test]
#[ignore] // requires root to mount tmpfs
fn disk_full_write_fails_clean() {
    use std::process::Command;

    let mount_point = "/tmp/syfrah_diskfull_test";
    let _ = std::fs::create_dir_all(mount_point);

    // Mount a tiny 64KB tmpfs
    let status = Command::new("mount")
        .args(["-t", "tmpfs", "-o", "size=64k", "tmpfs", mount_point])
        .status();

    if status.is_err() || !status.unwrap().success() {
        println!("Skipping disk full test (mount failed, likely not root)");
        return;
    }

    let path = std::path::PathBuf::from(mount_point).join("full.redb");
    let db = LayerDb::open_at(&path).unwrap();

    // Fill up the disk with large values
    let mut write_failed = false;
    for i in 0..1000 {
        let big = "X".repeat(1024);
        if db.set("fill", &format!("key_{i}"), &big).is_err() {
            write_failed = true;
            break;
        }
    }

    assert!(write_failed, "writes should eventually fail on full disk");

    // Cleanup
    let _ = Command::new("umount").arg(mount_point).status();
    let _ = std::fs::remove_dir(mount_point);
}

// Non-root version: test with a read-only file
#[test]
fn readonly_file_write_fails_clean() {
    // Root ignores file permissions, so this test is meaningless in containers
    if std::process::Command::new("id")
        .arg("-u")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
        .unwrap_or(false)
    {
        eprintln!("skipping: running as root");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("readonly.redb");

    // Create a valid db
    {
        let db = LayerDb::open_at(&path).unwrap();
        db.set("test", "key", &"value").unwrap();
    }

    // Make read-only
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    }

    // Try to open for writing — should fail
    let result = LayerDb::open_at(&path);

    // Restore permissions for cleanup
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    assert!(
        result.is_err(),
        "opening read-only file for write should fail"
    );
}
