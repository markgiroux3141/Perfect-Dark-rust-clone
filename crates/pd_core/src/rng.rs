//! PD's `random()` (`lib/rng_c.c`) and its seeding, verbatim. The world owns
//! exactly one stream, as PD does. The spike match had two (guns and bots), so a
//! seed did not pin down a match.
//!
//! Source: the old repo's `pd_spike/pdmath.rs` `Rng`.

/// PD's `random()` state: a 64-bit shift/xor generator whose low 32 bits are the result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    pub seed: u64,
}

impl Rng {
    /// `rng_set_seed`: PD adds 1 so the seed is never zero.
    pub fn new(seed: u64) -> Self {
        Rng { seed: seed.wrapping_add(1) }
    }

    /// `random()` (`rng_c.c:13`).
    pub fn random(&mut self) -> u32 {
        Self::rotate_seed(&mut self.seed)
    }

    /// `rng_rotate_seed` (`rng_c.c:35`): `random()`'s step applied to a seed the
    /// caller owns. C precedence: shifts, then `^`, then `|`, so `A | B ^ C` is
    /// `A | (B ^ C)`.
    pub fn rotate_seed(seed: &mut u64) -> u32 {
        let s = *seed;
        let s = ((s << 63) >> 31) | (((s << 31) >> 32) ^ ((s << 44) >> 32));
        let s = ((s >> 20) & 0xfff) ^ s;
        *seed = s;
        s as u32
    }

    /// `RANDOMFRAC()` = `random() * (1.0f / U32_MAX)`.
    pub fn randomfrac(&mut self) -> f32 {
        self.random() as f32 * (1.0 / u32::MAX as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rng_is_deterministic_and_spreads() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        let xs: Vec<u32> = (0..100).map(|_| a.random()).collect();
        let ys: Vec<u32> = (0..100).map(|_| b.random()).collect();
        assert_eq!(xs, ys);
        let mean = (0..10_000).map(|_| a.randomfrac()).sum::<f32>() / 10_000.0;
        assert!((mean - 0.5).abs() < 0.03, "{mean}");
    }

    #[test]
    fn random_and_rotate_seed_are_the_same_step() {
        let mut r = Rng::new(12345);
        let mut s = r.seed;
        for _ in 0..50 {
            assert_eq!(r.random(), Rng::rotate_seed(&mut s));
        }
    }

    #[test]
    fn the_first_draws_are_pds() {
        // rng_c.c:13 evaluated by hand (Python, 64-bit wrapping) from seeds 0 and 7.
        let mut z = Rng::new(0);
        assert_eq!([z.random(), z.random(), z.random()], [4096, 2_164_260_880, 1_082_196_992]);
        let mut s = Rng::new(7);
        assert_eq!([s.random(), s.random(), s.random()], [32772, 134_217_858, 67_641_345]);
    }
}
