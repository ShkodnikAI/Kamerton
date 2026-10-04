//! Integration tests for the arm library seeding (naryad Н1-04).
//!
//! Acceptance: the seed is idempotent — a repeat does not duplicate rows;
//! every arm carries a non-empty prior source; the grid is 5 x 2 x 2.

use kamerton::arms::{self, GRID_DURATIONS, GRID_METHODS, GRID_WINDOWS};
use kamerton::store::Store;

#[test]
fn seed_is_idempotent_and_grid_is_complete() {
    let s = Store::open_in_memory().unwrap();

    let first = arms::seed(&s, arms::SEEDS_JSON).unwrap();
    assert_eq!(
        first.inserted,
        GRID_METHODS * GRID_DURATIONS * GRID_WINDOWS,
        "first run inserts the whole grid"
    );
    assert_eq!(first.skipped, 0);

    let second = arms::seed(&s, arms::SEEDS_JSON).unwrap();
    assert_eq!(second.inserted, 0, "repeat run must insert nothing");
    assert_eq!(second.skipped, GRID_METHODS * GRID_DURATIONS * GRID_WINDOWS);

    let count: i64 = s
        .conn()
        .query_row("SELECT count(*) FROM arm", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count as usize, GRID_METHODS * GRID_DURATIONS * GRID_WINDOWS);
}

#[test]
fn every_arm_has_nonempty_prior_source_and_valid_grid_key() {
    let s = Store::open_in_memory().unwrap();
    arms::seed(&s, arms::SEEDS_JSON).unwrap();

    let mut stmt = s
        .conn()
        .prepare(
            "SELECT method, duration_min, day_window, prior_source, prior_mean, prior_sd FROM arm",
        )
        .unwrap();
    let rows: Vec<(String, i64, String, String, f64, f64)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    let mut methods = std::collections::BTreeSet::new();
    let mut keys = std::collections::BTreeSet::new();
    for (method, duration, window, source, mean, sd) in rows {
        assert!(!source.trim().is_empty(), "empty prior_source for {method}");
        assert!(
            (0.0..=1.0).contains(&mean) && sd > 0.0,
            "prior out of range for {method}"
        );
        assert!(window == "morning" || window == "evening");
        assert!(duration == 5 || duration == 15);
        methods.insert(method.clone());
        keys.insert((method, duration, window));
    }
    assert_eq!(methods.len(), GRID_METHODS);
    assert_eq!(keys.len(), GRID_METHODS * GRID_DURATIONS * GRID_WINDOWS);
}

#[test]
fn third_party_client_reads_priors_without_documentation() {
    // Second method: read the seeded library back through a bare
    // connection with plain SQL only.
    let s = Store::open_in_memory().unwrap();
    arms::seed(&s, arms::SEEDS_JSON).unwrap();
    let resonant: f64 = s
        .conn()
        .query_row(
            "SELECT prior_mean FROM arm WHERE method='resonant_breathing' AND duration_min=15 AND day_window='evening'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let woop: f64 = s
        .conn()
        .query_row(
            "SELECT prior_mean FROM arm WHERE method='woop' AND duration_min=5 AND day_window='morning'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    // Literature mapping from the concept table: strong priors sit above
    // the weaker families.
    assert!(resonant > 0.5 && woop > 0.5);
}
