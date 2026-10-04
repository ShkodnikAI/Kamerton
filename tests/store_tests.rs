//! Integration tests for the storage layer (naryad Н1-03).
//!
//! Acceptance: migration applies and rolls back idempotently; export
//! produces six CSVs; second method — the schema is read with raw SQL
//! through a plain read-only connection, without any migration code.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use kamerton::store::csv;
use kamerton::store::Store;
use rusqlite::{Connection, OpenFlags};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("kamerton-{}-{}-{}", tag, std::process::id(), nanos));
    fs::create_dir_all(&dir).unwrap();
    let _ = COUNTER.fetch_add(1, Ordering::Relaxed);
    dir
}

/// Inserts one valid row into every table; returns ids.
fn seed_one_of_each(s: &Store) -> (i64, i64, i64, i64, i64, i64) {
    s.conn()
        .execute(
            "INSERT INTO note(text, created_at) VALUES ('зазор между желанием и действием', '2026-10-04T07:00:00Z')",
            [],
        )
        .unwrap();
    let note_id = s.conn().last_insert_rowid();
    s.conn()
        .execute(
            "INSERT INTO arm(method, duration_min, day_window, prior_mean, prior_sd, prior_source, created_at)
             VALUES ('resonant breathing', 10, 'morning', 0.55, 0.15, 'Lehrer & Gevirtz 2014, HRV resonance review', '2026-10-04T07:00:00Z')",
            [],
        )
        .unwrap();
    let arm_id = s.conn().last_insert_rowid();
    s.conn()
        .execute(
            "INSERT INTO trial(note_id, arm_id, started_at, predicted, selection_prob, is_exploration)
             VALUES (?1, ?2, '2026-10-04T07:00:00Z', 'к вечеру зазор уже, чем обычно', 0.41, 0)",
            rusqlite::params![note_id, arm_id],
        )
        .unwrap();
    let trial_id = s.conn().last_insert_rowid();
    s.conn()
        .execute(
            "INSERT INTO response(trial_id, kind, score, logged_at)
             VALUES (?1, 'immediate', 0.62, '2026-10-04T21:30:00Z')",
            rusqlite::params![trial_id],
        )
        .unwrap();
    let response_id = s.conn().last_insert_rowid();
    s.conn()
        .execute(
            "INSERT INTO context(trial_id, logged_at, day_window, sleep_quality, moon_phase, moon_day)
             VALUES (?1, '2026-10-04T07:00:00Z', 'morning', 4, 0.31, 9)",
            rusqlite::params![trial_id],
        )
        .unwrap();
    let context_id = s.conn().last_insert_rowid();
    s.conn()
        .execute(
            "INSERT INTO candidate(name, source, status, evidence) VALUES ('moon_phase', 'moon', 'candidate', 'flat prior, not promoted')",
            [],
        )
        .unwrap();
    let candidate_id = s.conn().last_insert_rowid();
    (
        note_id,
        arm_id,
        trial_id,
        response_id,
        context_id,
        candidate_id,
    )
}

#[test]
fn apply_rollback_reapply_is_idempotent() {
    let dir = tmp_dir("migrate");
    let db = dir.join("kamerton.db");

    let mut s = Store::open(&db).unwrap();
    assert_eq!(s.user_version().unwrap(), 1);
    // Second open on the same file: nothing pending.
    let mut s2 = Store::open(&db).unwrap();
    assert_eq!(s2.apply_migrations().unwrap(), 0);

    // Marker row, then rollback: tables disappear, version resets.
    seed_one_of_each(&s);
    s.rollback_to(0).unwrap();
    assert_eq!(s.user_version().unwrap(), 0);
    let tables: i64 = s
        .conn()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'
             AND name IN ('note','arm','trial','response','context','candidate')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0);

    // Rollback below zero is a no-op (idempotent).
    s.rollback_to(0).unwrap();
    assert_eq!(s.user_version().unwrap(), 0);

    // Reapply gives a fresh schema; reapply again changes nothing.
    assert_eq!(s.apply_migrations().unwrap(), 1);
    assert_eq!(s.apply_migrations().unwrap(), 0);
    let count: i64 = s
        .conn()
        .query_row("SELECT count(*) FROM note", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "reapplied schema must be empty");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn second_method_raw_schema_introspection() {
    // The store is created normally, then read back through a bare
    // read-only connection: no Store methods, no migration code involved.
    let dir = tmp_dir("raw");
    let db = dir.join("kamerton.db");
    {
        let _s = Store::open(&db).unwrap();
    }

    let conn = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let expected: &[(&str, &[&str])] = &[
        ("note", &["id", "text", "is_active", "created_at"]),
        (
            "arm",
            &[
                "id",
                "method",
                "duration_min",
                "day_window",
                "prior_mean",
                "prior_sd",
                "prior_source",
                "is_active",
                "created_at",
            ],
        ),
        (
            "trial",
            &[
                "id",
                "note_id",
                "arm_id",
                "started_at",
                "predicted",
                "selection_prob",
                "is_exploration",
            ],
        ),
        (
            "response",
            &["id", "trial_id", "kind", "score", "logged_at"],
        ),
        (
            "context",
            &[
                "id",
                "trial_id",
                "logged_at",
                "day_window",
                "sleep_quality",
                "moon_phase",
                "moon_day",
                "revealed_at",
            ],
        ),
        (
            "candidate",
            &["id", "name", "source", "status", "evidence", "decided_at"],
        ),
    ];
    for (table, cols) in expected {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        let names: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            &names, cols,
            "table {table} columns differ from the documented schema"
        );
    }

    // Foreign keys point where the concept says they must.
    let fk = |table: &str| -> Vec<(String, String)> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA foreign_key_list({table})"))
            .unwrap();
        stmt.query_map([], |r| Ok((r.get::<_, String>(3)?, r.get::<_, String>(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert!(fk("trial").contains(&("note_id".into(), "note".into())));
    assert!(fk("trial").contains(&("arm_id".into(), "arm".into())));
    assert!(fk("response").contains(&("trial_id".into(), "trial".into())));
    assert!(fk("context").contains(&("trial_id".into(), "trial".into())));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn export_csv_six_files_and_roundtrip() {
    let dir = tmp_dir("export");
    let s = Store::open_in_memory().unwrap();
    seed_one_of_each(&s);
    // A prior_source with a comma, quotes and a newline exercises quoting.
    s.conn()
        .execute(
            "INSERT INTO arm(method, duration_min, day_window, prior_mean, prior_sd, prior_source, created_at)
             VALUES ('WOOP', 5, 'evening', 0.5, 0.2,
                     'Gollwitzer 1999; \"implementation intentions\", meta-analysis
                      with a second line', '2026-10-04T07:00:00Z')",
            [],
        )
        .unwrap();

    let out = s.export_csv(&dir).unwrap();
    assert_eq!(out.len(), 6);

    for table in kamerton::store::TABLES {
        let path = dir.join(format!("{table}.csv"));
        assert!(path.exists(), "missing export for {table}");
        let content = fs::read_to_string(&path).unwrap();
        let recs = csv::records(&content);
        let header = recs[0].clone();
        let expected_header = s.column_names(table).unwrap();
        assert_eq!(&header, &expected_header, "header of {table}");

        let expected_rows: i64 = s
            .conn()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            recs.len() - 1,
            expected_rows as usize,
            "row count of {table}"
        );
        for row in &recs[1..] {
            assert_eq!(row.len(), header.len(), "field count of a {table} row");
        }
    }

    // The tricky prior_source survives the round trip byte for byte.
    let arm_csv = fs::read_to_string(dir.join("arm.csv")).unwrap();
    assert!(arm_csv.contains("\"Gollwitzer 1999; \"\"implementation intentions\"\", meta-analysis"));
    let tricky: String = s
        .conn()
        .query_row(
            "SELECT prior_source FROM arm WHERE method='WOOP'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(tricky.contains("\n"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn constraints_and_foreign_keys_are_enforced() {
    let s = Store::open_in_memory().unwrap();
    seed_one_of_each(&s);

    let bad = |sql: &str| {
        s.conn()
            .execute(sql, [])
            .err()
            .unwrap_or_else(|| panic!("expected violation: {sql}"))
    };

    // Response score out of range.
    bad("INSERT INTO response(trial_id, kind, score, logged_at)
         VALUES (1, 'shift24h', 1.5, '2026-10-05T21:30:00Z')");
    // Unknown response kind.
    bad("INSERT INTO response(trial_id, kind, score, logged_at)
         VALUES (1, 'sometime', 0.5, '2026-10-05T21:30:00Z')");
    // Duplicate (trial, kind).
    bad("INSERT INTO response(trial_id, kind, score, logged_at)
         VALUES (1, 'immediate', 0.1, '2026-10-05T21:30:00Z')");
    // Empty prior source (Н1-04 acceptance relies on this).
    bad("INSERT INTO arm(method, duration_min, day_window, prior_mean, prior_sd, prior_source, created_at)
         VALUES ('x', 5, 'morning', 0.5, 0.2, '', '2026-10-04T07:00:00Z')");
    // Foreign key to a missing arm.
    bad(
        "INSERT INTO trial(note_id, arm_id, started_at, predicted, selection_prob, is_exploration)
         VALUES (1, 999, '2026-10-04T07:00:00Z', 'p', 0.5, 0)",
    );
    // Context blind: revealed_at is NULL until a response opens it.
    let revealed: Option<String> = s
        .conn()
        .query_row("SELECT revealed_at FROM context WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(revealed, None);
}
