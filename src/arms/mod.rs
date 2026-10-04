//! Arm library: the first-wave protocol grid with literature-seeded priors.
//!
//! Grid: 5 methods x 2 durations x 2 day windows = 20 arms. Every arm
//! carries a prior (mean, sd) on the reward scale [0, 1] and a non-empty
//! `prior_source` pointing at the literature it is seeded from (Н1-04:
//! priors come from literature, not from zeros).

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::store::{Error, Result, Store};

pub const GRID_METHODS: usize = 5;
pub const GRID_DURATIONS: usize = 2;
pub const GRID_WINDOWS: usize = 2;

pub const SEEDS_JSON: &str = include_str!("../../seeds/arms.json");

/// Prior level bands on the reward scale. `weak_wide` is deliberately the
/// widest: the literature support is weakest, so the prior must stay open.
const LEVEL_BANDS: [(&str, f64, f64, f64); 4] = [
    // (level, mean_min, mean_max, sd_max)
    ("strong", 0.55, 0.65, 0.18),
    ("medium", 0.45, 0.55, 0.18),
    ("medium_weak", 0.40, 0.50, 0.20),
    ("weak_wide", 0.30, 0.45, 0.30),
];

#[derive(Debug, Deserialize)]
pub struct SeedsFile {
    pub methods: Vec<MethodSeed>,
}

#[derive(Debug, Deserialize)]
pub struct MethodSeed {
    pub method: String,
    #[allow(dead_code)]
    pub label: String,
    pub duration_minutes: Vec<u32>,
    pub day_windows: Vec<String>,
    pub prior_level: String,
    pub prior_mean: f64,
    pub prior_sd: f64,
    pub prior_source: String,
}

/// One fully expanded arm row, ready for the `arm` table.
#[derive(Debug, PartialEq, Clone)]
pub struct ArmRow {
    pub method: String,
    pub duration_min: u32,
    pub day_window: String,
    pub prior_mean: f64,
    pub prior_sd: f64,
    pub prior_source: String,
}

/// Summary of one seeding run.
#[derive(Debug, PartialEq, Eq)]
pub struct SeedReport {
    pub inserted: usize,
    pub skipped: usize,
}

/// Parses and validates the seeds document.
pub fn load_seeds(json: &str) -> Result<Vec<MethodSeed>> {
    let file: SeedsFile =
        serde_json::from_str(json).map_err(|e| Error::Io(std::io::Error::other(e)))?;
    let mut seen = BTreeSet::new();
    for m in &file.methods {
        if !seen.insert(m.method.clone()) {
            return Err(Error::Io(std::io::Error::other(format!(
                "duplicate method in seeds: {}",
                m.method
            ))));
        }
        if m.prior_source.trim().is_empty() {
            return Err(Error::Io(std::io::Error::other(format!(
                "empty prior_source for {} (Н1-04: every arm needs a source)",
                m.method
            ))));
        }
        let band = LEVEL_BANDS
            .iter()
            .find(|(level, _, _, _)| *level == m.prior_level)
            .ok_or_else(|| {
                Error::Io(std::io::Error::other(format!(
                    "unknown prior_level '{}' for {}",
                    m.prior_level, m.method
                )))
            })?;
        let (_, mean_min, mean_max, sd_max) = *band;
        if !(mean_min..=mean_max).contains(&m.prior_mean) {
            return Err(Error::Io(std::io::Error::other(format!(
                "prior_mean {} out of band [{mean_min}, {mean_max}] for level '{}' ({})",
                m.prior_mean, m.prior_level, m.method
            ))));
        }
        if !(0.0 < m.prior_sd && m.prior_sd <= sd_max) {
            return Err(Error::Io(std::io::Error::other(format!(
                "prior_sd {} out of band (0, {sd_max}] for level '{}' ({})",
                m.prior_sd, m.prior_level, m.method
            ))));
        }
    }
    if file.methods.len() != GRID_METHODS {
        return Err(Error::Io(std::io::Error::other(format!(
            "expected {GRID_METHODS} methods, got {}",
            file.methods.len()
        ))));
    }
    Ok(file.methods)
}

/// Expands methods into the full deterministic grid: 5 x 2 x 2 = 20 rows,
/// ordered by method, duration, window.
pub fn expand(seeds: &[MethodSeed]) -> Vec<ArmRow> {
    let mut rows = Vec::with_capacity(seeds.len() * GRID_DURATIONS * GRID_WINDOWS);
    for m in seeds {
        for &d in &m.duration_minutes {
            for w in &m.day_windows {
                rows.push(ArmRow {
                    method: m.method.clone(),
                    duration_min: d,
                    day_window: w.clone(),
                    prior_mean: m.prior_mean,
                    prior_sd: m.prior_sd,
                    prior_source: m.prior_source.clone(),
                });
            }
        }
    }
    rows
}

/// Seeds the arm library. Idempotent: a second run inserts nothing
/// (`INSERT OR IGNORE` against the UNIQUE(method, duration, window) key).
pub fn seed(store: &Store, json: &str) -> Result<SeedReport> {
    let rows = expand(&load_seeds(json)?);
    let mut inserted = 0;
    let mut skipped = 0;
    for r in &rows {
        let changed = store.conn().execute(
            "INSERT OR IGNORE INTO arm(method, duration_min, day_window,
                                       prior_mean, prior_sd, prior_source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                r.method,
                r.duration_min,
                r.day_window,
                r.prior_mean,
                r.prior_sd,
                r.prior_source,
                crate::store::time::now_iso()
            ],
        )?;
        if changed == 1 {
            inserted += 1;
        } else {
            skipped += 1;
        }
    }
    Ok(SeedReport { inserted, skipped })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_seeds_are_valid() {
        let seeds = load_seeds(SEEDS_JSON).unwrap();
        assert_eq!(seeds.len(), GRID_METHODS);
        for m in &seeds {
            assert_eq!(m.duration_minutes.len(), GRID_DURATIONS);
            assert_eq!(m.day_windows.len(), GRID_WINDOWS);
            assert!(!m.prior_source.trim().is_empty());
        }
    }

    #[test]
    fn expand_produces_full_grid() {
        let seeds = load_seeds(SEEDS_JSON).unwrap();
        let rows = expand(&seeds);
        assert_eq!(rows.len(), GRID_METHODS * GRID_DURATIONS * GRID_WINDOWS);
        let methods: BTreeSet<&str> = rows.iter().map(|r| r.method.as_str()).collect();
        assert_eq!(methods.len(), GRID_METHODS);
        let mut keyed = BTreeSet::new();
        for r in &rows {
            assert!(keyed.insert((r.method.clone(), r.duration_min, r.day_window.clone())));
            assert!((0.0..=1.0).contains(&r.prior_mean));
            assert!(r.prior_sd > 0.0);
            assert!(!r.prior_source.is_empty());
        }
    }

    #[test]
    fn rejects_empty_source_and_bad_bands() {
        let bad_source = r#"{"methods":[{"method":"x","label":"x","duration_minutes":[5,15],
            "day_windows":["morning","evening"],"prior_level":"strong","prior_mean":0.6,
            "prior_sd":0.15,"prior_source":"  "}]}"#;
        assert!(load_seeds(bad_source).is_err());

        let bad_band = r#"{"methods":[{"method":"x","label":"x","duration_minutes":[5,15],
            "day_windows":["morning","evening"],"prior_level":"weak_wide","prior_mean":0.9,
            "prior_sd":0.25,"prior_source":"real source"}]}"#;
        assert!(load_seeds(bad_band).is_err());

        let bad_sd = r#"{"methods":[{"method":"x","label":"x","duration_minutes":[5,15],
            "day_windows":["morning","evening"],"prior_level":"strong","prior_mean":0.6,
            "prior_sd":0.5,"prior_source":"real source"}]}"#;
        assert!(load_seeds(bad_sd).is_err());
    }
}
