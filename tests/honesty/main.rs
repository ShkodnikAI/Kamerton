//! Integration tests for the honesty valve (naryad Н1-08).
//!
//! Acceptance: on synthetic data without signal the report says plainly
//! "сигнала нет"; on synthetic data with signal it shows the gain; every
//! estimate is a range (low / expected / high), never a point.

use kamerton::honesty::{self, HonestyConfig, TrialRecord, Verdict};
use kamerton::report::format_report;
use kamerton::store::{time, Store};

/// Simulates logged trials: a policy that concentrates on the best arm
/// with propensity `p_best` (the rest spread uniformly), rewards drawn
/// around each arm's true mean.
fn simulate(
    true_means: &[f64],
    p_best: f64,
    best_index: usize,
    noise: f64,
    seed: u64,
    n: usize,
) -> Vec<TrialRecord> {
    use kamerton::bandit::rng::NormalSampler;
    let mut rng = NormalSampler::new(seed);
    let k = true_means.len();
    let mut records = Vec::with_capacity(n);
    for _ in 0..n {
        // Behavior policy: best arm with prob p_best, else uniform over
        // the rest. The propensity of the executed choice is exact.
        let coin = rng.uniform();
        let (arm, selection_prob) = if coin < p_best {
            (best_index, p_best)
        } else {
            let rest = k - 1;
            let slot = rng.uniform();
            let idx = ((slot * rest as f64) as usize).min(rest - 1);
            let arm = if idx >= best_index { idx + 1 } else { idx };
            let prob = (1.0 - p_best) / rest as f64;
            (arm, prob)
        };
        // The reward belongs to the CHOSEN arm: this is what the valve
        // sees, and the IPW estimator relies on it.
        let reward = (true_means[arm] + noise * rng.normal()).clamp(0.0, 1.0);
        records.push(TrialRecord {
            reward,
            selection_prob,
        });
    }
    records
}

#[test]
fn no_signal_world_says_plainly_there_is_no_signal() {
    // All arms are identical: no protocol can beat a coin toss.
    let means = [0.5f64; 20];
    let records = simulate(&means, 0.5, 3, 0.15, 42, 400);
    let config = HonestyConfig {
        arm_count: 20,
        ..HonestyConfig::default()
    };
    let report = honesty::evaluate(&records, &config);
    assert_eq!(report.verdict, Verdict::NoSignal);
    let text = format_report(&report);
    assert!(text.contains("сигнала нет"), ":\n{text}");
}

#[test]
fn signal_world_shows_the_gain() {
    // One arm is genuinely better and the policy concentrated on it.
    let mut means = [0.5f64; 20];
    means[7] = 0.85;
    let records = simulate(&means, 0.6, 7, 0.10, 4242, 600);
    let config = HonestyConfig {
        arm_count: 20,
        ..HonestyConfig::default()
    };
    let report = honesty::evaluate(&records, &config);
    assert_eq!(report.verdict, Verdict::Signal);
    assert!(report.edge.low > 0.0, "edge = {report:?}");
    let text = format_report(&report);
    assert!(text.contains("выигрыш"), ":\n{text}");
}

#[test]
fn every_estimate_is_a_range_not_a_point() {
    let means = [0.5f64; 20];
    let records = simulate(&means, 0.5, 0, 0.15, 7, 200);
    let config = HonestyConfig {
        arm_count: 20,
        ..HonestyConfig::default()
    };
    let report = honesty::evaluate(&records, &config);
    for (name, r) in [
        ("bandit", report.bandit),
        ("random", report.random),
        ("edge", report.edge),
    ] {
        assert!(r.low <= r.expected && r.expected <= r.high, "{name}: {r:?}");
        assert!(r.low < r.high, "{name} degenerated to a point: {r:?}");
    }
    let text = format_report(&report);
    assert!(
        text.contains("от ") && text.contains(" до "),
        "ranges in text:\n{text}"
    );
}

#[test]
fn report_is_reproducible_with_fixed_seed() {
    let mut means = [0.5f64; 20];
    means[2] = 0.8;
    let records = simulate(&means, 0.55, 2, 0.12, 99, 300);
    let config = HonestyConfig {
        arm_count: 20,
        ..HonestyConfig::default()
    };
    let a = honesty::evaluate(&records, &config);
    let b = honesty::evaluate(&records, &config);
    assert_eq!(a, b, "same seed and data must give the same report");
}

#[test]
fn loader_takes_only_matured_trials_and_weights_responses() {
    let s = Store::open_in_memory().unwrap();
    s.conn()
        .execute(
            "INSERT INTO note(text, created_at) VALUES ('n', '2026-10-01T07:00:00Z')",
            [],
        )
        .unwrap();
    let note = s.conn().last_insert_rowid();
    s.conn()
        .execute(
            "INSERT INTO arm(method, duration_min, day_window, prior_mean, prior_sd, prior_source, created_at)
             VALUES ('m', 5, 'morning', 0.5, 0.2, 'src', '2026-10-01T07:00:00Z')",
            [],
        )
        .unwrap();
    let arm = s.conn().last_insert_rowid();

    let mk_trial = |s: &Store, started_at: &str, prob: f64| -> i64 {
        s.conn()
            .execute(
                "INSERT INTO trial(note_id, arm_id, started_at, predicted, selection_prob, is_exploration)
                 VALUES (?1, ?2, ?3, 'p', ?4, 0)",
                rusqlite::params![note, arm, started_at, prob],
            )
            .unwrap();
        s.conn().last_insert_rowid()
    };
    let mk_response = |s: &Store, trial: i64, kind: &str, score: f64| {
        s.conn()
            .execute(
                "INSERT INTO response(trial_id, kind, score, logged_at) VALUES (?1, ?2, ?3, ?2)",
                rusqlite::params![trial, kind, score],
            )
            .unwrap();
    };

    // Matured trial (started 4 days ago) with all three kinds:
    // reward = 0.2*0.5 + 0.3*0.6 + 0.5*0.8 = 0.68.
    let t1 = mk_trial(&s, "2026-10-01T07:00:00Z", 0.4);
    mk_response(&s, t1, "immediate", 0.5);
    mk_response(&s, t1, "shift24h", 0.6);
    mk_response(&s, t1, "action72h", 0.8);

    // Immature trial (started 1 hour ago): excluded even with responses.
    let t2 = mk_trial(&s, "2026-10-05T06:00:00Z", 0.9);
    mk_response(&s, t2, "immediate", 0.99);

    // Matured but response-less: no rows joined -> no trial record.
    let _t3 = mk_trial(&s, "2026-10-02T07:00:00Z", 0.2);

    let now = almanac("2026-10-05T07:00:00Z");
    let records = honesty::load_matured_trials(&s, now).unwrap();
    assert_eq!(records.len(), 1, "only matured trials with responses");
    let rec = &records[0];
    assert!((rec.reward - 0.68).abs() < 1e-9, "reward = {}", rec.reward);
    assert!((rec.selection_prob - 0.4).abs() < 1e-9);
}

fn almanac(iso: &str) -> i64 {
    time::unix_from_iso(iso).unwrap()
}
