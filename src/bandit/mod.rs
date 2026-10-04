//! The selection engine: Thompson sampling on Gaussian posteriors.
//!
//! Concept mapping (section "Trial, data, engine" and naryad Н1-05):
//! - each arm carries a Gaussian posterior over its reward in [0, 1];
//! - selection: draw one sample per arm, pick the argmax (Thompson);
//! - forced exploration: at least 20% of trials pick a uniformly random
//!   arm (invariant 5 — exploration never switches off);
//! - responses older than `decay_days` lose weight exponentially
//!   (the model must be allowed to change its mind about the person
//!   the owner was last spring).
//!
//! Determinism: one shared Box-Muller stream drives every stochastic
//! decision. A fixed seed reproduces a whole run byte for byte. Rebuilding
//! posteriors from history never touches the stream.

pub mod rng;

use crate::bandit::rng::NormalSampler;

/// Engine configuration. Every number here is a decision of the concept,
/// not a tuning knob to play with at runtime.
#[derive(Debug, Clone)]
pub struct BanditConfig {
    /// Shared PRNG seed. A fixed seed reproduces a whole run.
    pub seed: u64,
    /// Forced exploration rate (invariant 5: minimum 20%).
    pub exploration_rate: f64,
    /// Responses older than this many days start losing weight.
    pub decay_days: f64,
    /// Exponential time constant of the weight loss after `decay_days`.
    pub tau_days: f64,
    /// Known observation noise of a single response (sigma).
    pub obs_sigma: f64,
    /// Monte-Carlo draws for the propensity estimate.
    pub mc_draws: usize,
    /// When false, decay is disabled. Used only by the mutation test that
    /// proves the suite catches a removed decay.
    pub decay_enabled: bool,
}

impl Default for BanditConfig {
    fn default() -> Self {
        BanditConfig {
            seed: 20_260_410,
            exploration_rate: 0.20,
            decay_days: 60.0,
            tau_days: 60.0,
            obs_sigma: 0.25,
            mc_draws: 64,
            decay_enabled: true,
        }
    }
}

/// Weight of a response observed `age_days` ago. Full weight up to
/// `decay_days`, then exponential loss with time constant `tau_days`.
pub fn decay_weight(config: &BanditConfig, age_days: f64) -> f64 {
    if !config.decay_enabled || age_days <= config.decay_days {
        1.0
    } else {
        (-(age_days - config.decay_days) / config.tau_days).exp()
    }
}

/// Conjugate Normal posterior over an arm's mean reward.
#[derive(Debug, Clone, PartialEq)]
pub struct Posterior {
    pub mean: f64,
    pub sd: f64,
}

/// One observed response: reward in [0, 1] and how many days ago the
/// response happened, relative to "now" of the rebuild.
#[derive(Debug, Clone, Copy)]
pub struct Observation {
    pub arm_index: usize,
    pub reward: f64,
    pub age_days: f64,
}

/// A selection decision with the propensity needed by the honesty valve
/// for off-policy evaluation (Н1-08).
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub arm_index: usize,
    pub is_exploration: bool,
    /// Behavior-policy probability of this choice:
    /// e/K + (1 - e) * P(Thompson picks this arm). Stored on the trial row.
    pub selection_prob: f64,
}

/// The deterministic selection engine.
#[derive(Debug, Clone)]
pub struct Bandit {
    config: BanditConfig,
    rng: NormalSampler,
    priors: Vec<(f64, f64)>,
    posteriors: Vec<Posterior>,
}

impl Bandit {
    /// Creates the engine on arm priors; the PRNG stream starts here.
    pub fn new(priors: &[(f64, f64)], config: BanditConfig) -> Self {
        assert!(!priors.is_empty(), "bandit needs at least one arm");
        assert!(
            (0.0..1.0).contains(&config.exploration_rate),
            "exploration rate must be in [0, 1)"
        );
        let posteriors = priors
            .iter()
            .map(|(m, s)| Posterior { mean: *m, sd: *s })
            .collect();
        let rng = NormalSampler::new(config.seed);
        Bandit {
            config,
            rng,
            priors: priors.to_vec(),
            posteriors,
        }
    }

    /// Cold start from a full history: posteriors are recomputed with
    /// decay weights relative to now. Same as `new` + `rebuild`.
    pub fn build(priors: &[(f64, f64)], history: &[Observation], config: BanditConfig) -> Self {
        let mut bandit = Self::new(priors, config);
        bandit.rebuild(history);
        bandit
    }

    pub fn config(&self) -> &BanditConfig {
        &self.config
    }

    pub fn posteriors(&self) -> &[Posterior] {
        &self.posteriors
    }

    /// Recomputes every posterior from the ORIGINAL priors and the full
    /// history with decay weights relative to the current moment. The PRNG
    /// stream is not touched. Rebuilding must never blend the current
    /// posterior in as a prior: that would double-count observations that
    /// are already in it.
    pub fn rebuild(&mut self, history: &[Observation]) {
        let k = self.posteriors.len();
        let mut weighted_rewards = vec![0.0f64; k];
        let mut weights = vec![0.0f64; k];
        for obs in history {
            let w = decay_weight(&self.config, obs.age_days);
            weighted_rewards[obs.arm_index] += w * obs.reward;
            weights[obs.arm_index] += w;
        }
        for (i, p) in self.posteriors.iter_mut().enumerate() {
            let (m0, s0) = self.priors[i];
            let p0 = 1.0 / (s0 * s0);
            let p_post = p0 + weights[i] / (self.config.obs_sigma * self.config.obs_sigma);
            let m_post = (p0 * m0
                + weighted_rewards[i] / (self.config.obs_sigma * self.config.obs_sigma))
                / p_post;
            p.mean = m_post;
            p.sd = 1.0 / p_post.sqrt();
        }
    }

    /// Incremental conjugate update with one response; the weight is the
    /// decay weight of its age at ingestion time.
    pub fn update(&mut self, arm_index: usize, reward: f64, age_days: f64) {
        let w =
            decay_weight(&self.config, age_days) / (self.config.obs_sigma * self.config.obs_sigma);
        let p = &mut self.posteriors[arm_index];
        let p0 = 1.0 / (p.sd * p.sd);
        let m0 = p.mean;
        let p_post = p0 + w;
        let m_post = (p0 * m0 + w * reward) / p_post;
        p.mean = m_post;
        p.sd = 1.0 / p_post.sqrt();
    }

    /// Monte-Carlo estimate of P(Thompson picks arm k), deterministic for
    /// a fixed seed; used for the behavior propensity of a selection.
    fn thompson_probs(&mut self) -> Vec<f64> {
        let k = self.posteriors.len();
        let mut wins = vec![0usize; k];
        for _ in 0..self.config.mc_draws {
            let mut best = 0usize;
            let mut best_val = f64::NEG_INFINITY;
            for (i, p) in self.posteriors.iter().enumerate() {
                let v = self.rng.normal_mu_sigma(p.mean, p.sd);
                if v > best_val {
                    best_val = v;
                    best = i;
                }
            }
            wins[best] += 1;
        }
        wins.into_iter()
            .map(|c| c as f64 / self.config.mc_draws as f64)
            .collect()
    }

    /// Picks an arm for the next trial. `exploration_rate` of trials are
    /// forced to a uniformly random arm (invariant 5); the rest go through
    /// Thompson sampling.
    pub fn select(&mut self) -> Selection {
        let k = self.posteriors.len();
        let e = self.config.exploration_rate;
        let probs = self.thompson_probs();

        let coin = self.rng.uniform();
        let (arm_index, is_exploration) = if coin < e {
            let slot = self.rng.uniform();
            let idx = ((slot * k as f64) as usize).min(k - 1);
            (idx, true)
        } else {
            let mut best = 0usize;
            let mut best_val = f64::NEG_INFINITY;
            for (i, p) in self.posteriors.iter().enumerate() {
                let v = self.rng.normal_mu_sigma(p.mean, p.sd);
                if v > best_val {
                    best_val = v;
                    best = i;
                }
            }
            (best, false)
        };

        let selection_prob = e / k as f64 + (1.0 - e) * probs[arm_index];
        Selection {
            arm_index,
            is_exploration,
            selection_prob,
        }
    }
}
