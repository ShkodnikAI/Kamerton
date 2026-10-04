//! Integration tests for the selection engine (naryad Н1-05).
//!
//! Acceptance: convergence to the best arm on synthetic data; exploration
//! share on 1000 runs inside the 20-25% corridor; the mutation check that
//! catches a removed decay; a fixed seed reproduces a run; a pinned digest
//! of the reference run for the TW/VM parity check.

use kamerton::bandit::{Bandit, BanditConfig, Observation};

/// A synthetic world: every arm has a true mean reward, observations are
/// noisy draws around it.
struct World {
    true_means: Vec<f64>,
    noise: f64,
    rng: kamerton::bandit::rng::NormalSampler,
}

impl World {
    fn new(true_means: Vec<f64>, noise: f64, seed: u64) -> Self {
        World {
            true_means,
            noise,
            rng: kamerton::bandit::rng::NormalSampler::new(seed),
        }
    }

    fn reward(&mut self, arm: usize) -> f64 {
        let m = self.true_means[arm];
        (m + self.noise * self.rng.normal()).clamp(0.0, 1.0)
    }
}

#[test]
fn converges_to_the_best_arm_on_synthetic() {
    let config = BanditConfig {
        seed: 1_001,
        ..BanditConfig::default()
    };
    let priors: Vec<(f64, f64)> = vec![(0.5, 0.2); 4];
    let mut world = World::new(vec![0.30, 0.62, 0.45, 0.50], 0.10, 777);
    let mut bandit = Bandit::new(&priors, config);

    let trials = 1500;
    let last_window = 300usize;
    let mut best_in_last = 0usize;
    let mut total_reward = 0.0;
    for t in 0..trials {
        let sel = bandit.select();
        let reward = world.reward(sel.arm_index);
        bandit.update(sel.arm_index, reward, 0.0);
        total_reward += reward;
        if t >= trials - last_window && sel.arm_index == 1 {
            best_in_last += 1;
        }
    }

    let share = best_in_last as f64 / last_window as f64;
    assert!(
        share >= 0.6,
        "best arm share in the last window must be >= 0.6, got {share}"
    );
    // The engine must also beat a uniform random policy in expectation:
    // mean of the world = (0.30 + 0.62 + 0.45 + 0.50) / 4 = 0.4675.
    assert!(
        total_reward / trials as f64 > 0.50,
        "average reward must beat the uniform policy"
    );
}

#[test]
fn exploration_share_in_corridor_on_1000_runs() {
    // 20 arms = the real first-wave grid size.
    let config = BanditConfig {
        seed: 20_260_410,
        ..BanditConfig::default()
    };
    let priors: Vec<(f64, f64)> = vec![(0.5, 0.2); 20];
    let mut bandit = Bandit::new(&priors, config);

    let n = 1000;
    let forced = (0..n).filter(|_| bandit.select().is_exploration).count();
    let share = forced as f64 / n as f64;
    assert!(
        (0.20..=0.25).contains(&share),
        "exploration share {share} must stay in the 20-25% corridor"
    );
}

/// The decay mutation scenario. With decay enabled the engine must switch
/// to the steady arm after arm 0 turns bad; with decay removed the stale
/// average keeps arm 0 on top. Both assertions live here, so deleting the
/// decay mechanism makes this test fail.
#[test]
fn mutation_removed_decay_is_caught() {
    let run = |decay_enabled: bool| -> Vec<usize> {
        let config = BanditConfig {
            seed: 5_005,
            decay_enabled,
            ..BanditConfig::default()
        };
        let priors: Vec<(f64, f64)> = vec![(0.5, 0.25); 2];
        let mut bandit = Bandit::new(&priors, config);
        let mut all_history: Vec<Observation> = Vec::new();
        let mut picks: Vec<usize> = Vec::new();
        let days_total = 240;

        for day in 0..days_total {
            let sel = bandit.select();
            picks.push(sel.arm_index);
            // Arm 0: reward 0.9 for 150 days, then 0.05. Arm 1: steady 0.52.
            // The naive average of arm 0 stays at ~0.58 (above arm 1), the
            // decayed one drops to ~0.29 (far below it).
            let reward = if sel.arm_index == 0 {
                if day < 150 {
                    0.9
                } else {
                    0.05
                }
            } else {
                0.52
            };
            bandit.update(sel.arm_index, reward, 0.0);
            all_history.push(Observation {
                arm_index: sel.arm_index,
                reward,
                age_days: 0.0,
            });
            // Daily rebuild against "now" = end of the current day.
            let now_day = day + 1;
            for (i, obs) in all_history.iter_mut().enumerate() {
                obs.age_days = (now_day - i) as f64; // one trial per day
            }
            bandit.rebuild(&all_history);
            if day % 10 == 0 && decay_enabled {
                eprintln!(
                    "day {day}: p0 {:.4} p1 {:.4}",
                    bandit.posteriors()[0].mean,
                    bandit.posteriors()[1].mean
                );
            }
        }
        picks
    };

    let days_total = 240;
    let window = 30;
    let with_decay = run(true);
    let without_decay = run(false);

    let arm1_with = with_decay[days_total - window..]
        .iter()
        .filter(|&&a| a == 1)
        .count();
    let arm0_without = without_decay[days_total - window..]
        .iter()
        .filter(|&&a| a == 0)
        .count();
    assert!(
        arm1_with >= 24,
        "with decay the engine must switch to the steady arm (got {arm1_with}/{window})"
    );
    assert!(
        arm0_without >= 24,
        "without decay the stale average must keep arm 0 on top (got {arm0_without}/{window})"
    );
}

#[test]
fn fixed_seed_reproduces_the_run() {
    let run = || -> (Vec<usize>, Vec<[u64; 2]>) {
        let config = BanditConfig {
            seed: 9_009,
            ..BanditConfig::default()
        };
        let priors: Vec<(f64, f64)> = vec![(0.5, 0.2); 20];
        let mut bandit = Bandit::new(&priors, config);
        let mut world = World::new(vec![0.35; 20], 0.15, 4_242);
        let mut picks = Vec::new();
        let mut posts: Vec<[u64; 2]> = Vec::new();
        for t in 0..500 {
            let sel = bandit.select();
            let reward = world.reward(sel.arm_index);
            bandit.update(sel.arm_index, reward, 0.0);
            picks.push(sel.arm_index);
            if t % 100 == 99 {
                let digest = bandit.posteriors().iter().fold([0u64; 2], |acc, p| {
                    [
                        acc[0].wrapping_add(p.mean.to_bits()),
                        acc[1].wrapping_add(p.sd.to_bits()),
                    ]
                });
                posts.push(digest);
            }
        }
        (picks, posts)
    };

    let (picks_a, posts_a) = run();
    let (picks_b, posts_b) = run();
    assert_eq!(picks_a, picks_b, "selection stream must reproduce exactly");
    assert_eq!(posts_a, posts_b, "posteriors must reproduce bit for bit");
}
