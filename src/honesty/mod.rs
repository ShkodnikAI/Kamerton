//! The honesty valve (invariant 4, naryad Н1-08).
//!
//! Without this module the system degenerates into a coincidence
//! generator: it compares the accumulated reward of the bandit's choices
//! against the expected reward of a uniform random policy on the same
//! trials, and it says plainly "сигнала нет" when the edge is not there.
//!
//! Method: inverse-propensity weighting. Every trial row carries the
//! behavior propensity `selection_prob` of the executed choice (written
//! by the bandit at selection time). For the uniform policy pi(a) = 1/K
//! the off-policy value estimate is
//!     V_random = mean_i( r_i * (1/K) / b_i ),
//! which is unbiased when the b_i are the true propensities. All
//! estimates come out as ranges (low / expected / high) from a seeded
//! bootstrap — the iron rule forbids point estimates.

use crate::store::{time, Store};

#[derive(Debug, Clone)]
pub struct HonestyConfig {
    /// Number of active arms in the grid (K of the uniform policy).
    pub arm_count: usize,
    /// Bootstrap resamples for the ranges.
    pub bootstrap_draws: usize,
    /// Seed of the bootstrap PRNG; a fixed seed reproduces the report.
    pub seed: u64,
}

impl Default for HonestyConfig {
    fn default() -> Self {
        HonestyConfig {
            arm_count: 20,
            bootstrap_draws: 2000,
            seed: 20_260_408,
        }
    }
}

/// One matured trial as the valve sees it: the total reward of the trial
/// (0.2 * immediate + 0.3 * shift24h + 0.5 * action72h, missing = 0) and
/// the propensity of the executed choice.
#[derive(Debug, Clone, Copy)]
pub struct TrialRecord {
    pub reward: f64,
    pub selection_prob: f64,
}

/// A range estimate: low / expected / high percentiles of a bootstrap
/// distribution. Never collapses to a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    pub low: f64,
    pub expected: f64,
    pub high: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The whole edge interval is above zero.
    Signal,
    /// The interval covers zero: no demonstrated gain.
    NoSignal,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HonestyReport {
    pub n_trials: usize,
    pub bandit: Range,
    pub random: Range,
    pub edge: Range,
    pub verdict: Verdict,
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    let idx = (p * (n - 1) as f64).round() as usize;
    sorted[idx.clamp(0, n - 1)]
}

fn make_range(samples: &mut [f64]) -> Range {
    samples.sort_by(|a, b| a.partial_cmp(b).expect("no NaN in bootstrap"));
    Range {
        low: percentile(samples, 0.05),
        expected: percentile(samples, 0.50),
        high: percentile(samples, 0.95),
    }
}

/// A minimal deterministic PRNG for the bootstrap stream (splitmix64).
struct BootRng {
    state: u64,
}

impl BootRng {
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// The verdict of the valve on a set of matured trials.
pub fn evaluate(records: &[TrialRecord], config: &HonestyConfig) -> HonestyReport {
    assert!(!records.is_empty(), "no matured trials to evaluate");
    assert!(config.arm_count > 0, "arm count must be positive");
    let n = records.len();
    let k = config.arm_count as f64;

    let mut rng = BootRng { state: config.seed };
    let mut bandit_samples = Vec::with_capacity(config.bootstrap_draws);
    let mut random_samples = Vec::with_capacity(config.bootstrap_draws);
    let mut edge_samples = Vec::with_capacity(config.bootstrap_draws);

    for _ in 0..config.bootstrap_draws {
        let mut sum_bandit = 0.0;
        let mut sum_random = 0.0;
        for _ in 0..n {
            let rec = records[rng.below(n)];
            sum_bandit += rec.reward;
            // Uniform policy value through inverse-propensity weighting.
            sum_random += rec.reward * (1.0 / k) / rec.selection_prob;
        }
        let b = sum_bandit / n as f64;
        let r = sum_random / n as f64;
        bandit_samples.push(b);
        random_samples.push(r);
        edge_samples.push(b - r);
    }

    let bandit = make_range(&mut bandit_samples);
    let random = make_range(&mut random_samples);
    let edge = make_range(&mut edge_samples);

    let verdict = if edge.low > 0.0 {
        Verdict::Signal
    } else {
        Verdict::NoSignal
    };

    HonestyReport {
        n_trials: n,
        bandit,
        random,
        edge,
        verdict,
    }
}

/// Loads matured trials from the store. A trial is matured when 72 hours
/// have passed since it started; its reward is
/// 0.2 * immediate + 0.3 * shift24h + 0.5 * action72h, with missing
/// response kinds contributing zero (a missed action is still data).
pub fn load_matured_trials(store: &Store, now_unix: i64) -> crate::store::Result<Vec<TrialRecord>> {
    let cutoff_iso = time::iso_from_unix(now_unix - 72 * 3600);
    let mut stmt = store.conn().prepare(
        "SELECT t.id, t.selection_prob, r.kind, r.score
         FROM trial t
         JOIN response r ON r.trial_id = t.id
         WHERE t.started_at <= ?1
         ORDER BY t.id",
    )?;
    let rows = stmt.query_map([&cutoff_iso], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, f64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, f64>(3)?,
        ))
    })?;

    // Aggregate responses per trial.
    let mut order: Vec<i64> = Vec::new();
    let mut props: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
    let mut rewards: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
    for row in rows {
        let (id, prop, kind, score) = row?;
        if let std::collections::hash_map::Entry::Vacant(v) = props.entry(id) {
            order.push(id);
            v.insert(prop);
            rewards.insert(id, 0.0);
        }
        let weight = match kind.as_str() {
            "immediate" => 0.2,
            "shift24h" => 0.3,
            "action72h" => 0.5,
            other => {
                return Err(crate::store::Error::Io(std::io::Error::other(format!(
                    "unknown response kind: {other}"
                ))))
            }
        };
        *rewards.get_mut(&id).expect("id registered") += weight * score;
    }

    Ok(order
        .into_iter()
        .map(|id| TrialRecord {
            reward: rewards[&id],
            selection_prob: props[&id],
        })
        .collect())
}
