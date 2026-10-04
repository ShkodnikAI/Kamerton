//! Shared deterministic PRNG: splitmix64 with a Box-Muller normal
//! sampler on top. One stream is shared by every stochastic decision of
//! the engine, so a fixed seed reproduces a whole run byte for byte
//! (Н1-05: fixed seed gives a reproducible run).

/// splitmix64 counter-based generator.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        SplitMix64 { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform on [0, 1) with 2^-53 resolution.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }
}

/// Box-Muller transform over the shared PRNG. Generates standard normal
/// deviates in pairs; the unused member of each pair is cached, so the
/// stream stays deterministic and no uniforms are wasted.
#[derive(Debug, Clone)]
pub struct NormalSampler {
    rng: SplitMix64,
    spare: Option<f64>,
}

impl NormalSampler {
    pub fn new(seed: u64) -> Self {
        NormalSampler {
            rng: SplitMix64::new(seed),
            spare: None,
        }
    }

    pub fn uniform(&mut self) -> f64 {
        self.rng.next_f64()
    }

    /// Standard normal N(0, 1) via the polar-free Box-Muller transform.
    pub fn normal(&mut self) -> f64 {
        if let Some(v) = self.spare.take() {
            return v;
        }
        let u1 = self.rng.next_f64();
        let u2 = self.rng.next_f64();
        // u1 == 0 would blow up log; the generator maps 0 to [0,1) but a
        // defensive guard keeps the transform total.
        let u1 = if u1 <= f64::EPSILON { f64::EPSILON } else { u1 };
        let r = (-2.0 * u1.ln()).sqrt();
        let theta = std::f64::consts::TAU * u2;
        self.spare = Some(r * theta.sin());
        r * theta.cos()
    }

    /// Normal N(mu, sigma).
    pub fn normal_mu_sigma(&mut self, mu: f64, sigma: f64) -> f64 {
        mu + sigma * self.normal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_seed_is_reproducible() {
        let mut a = SplitMix64::new(42);
        let mut b = SplitMix64::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
            assert_eq!(a.next_f64(), b.next_f64());
        }
    }

    #[test]
    fn uniform_in_range_and_reasonably_spread() {
        let mut rng = SplitMix64::new(7);
        let (mut lo, mut hi, mut sum) = (1.0f64, 0.0f64, 0.0f64);
        for _ in 0..10_000 {
            let v = rng.next_f64();
            assert!((0.0..1.0).contains(&v));
            lo = lo.min(v);
            hi = hi.max(v);
            sum += v;
        }
        assert!(lo < 0.01 && hi > 0.99);
        assert!((sum / 10_000.0 - 0.5).abs() < 0.02);
    }

    #[test]
    fn normal_moments_on_fixed_seed() {
        let mut rng = NormalSampler::new(123);
        let n = 20_000;
        let mut sum = 0.0;
        let mut sum_sq = 0.0;
        for _ in 0..n {
            let x = rng.normal();
            sum += x;
            sum_sq += x * x;
        }
        let mean = sum / n as f64;
        let var = sum_sq / n as f64 - mean * mean;
        assert!(mean.abs() < 0.03, "mean {mean}");
        assert!((var - 1.0).abs() < 0.06, "variance {var}");
    }
}
