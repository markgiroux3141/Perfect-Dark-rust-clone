//! Python's `random.Random`, exactly: the Mersenne Twister, seeded the way CPython seeds it from
//! an int or a string, and the handful of calls the generator makes, each drawing the same
//! numbers in the same order as Python's.
//!
//! The Python generator names every random stream with a string ("40/vast/3": seed, room,
//! attempt), so a level is its seed and nothing else. Porting the generator rather than
//! replacing it is what keeps seed 40 the level it was. `tests/key/rng.json` holds Python's own
//! output for every call here.

use crate::sha512::sha512;

const N: usize = 624;
const M: usize = 397;

/// `random.Random(seed)`.
#[derive(Clone)]
pub struct PyRandom {
    mt: [u32; N],
    at: usize,
}

impl PyRandom {
    /// `random.Random(n)` for a whole number that is not negative.
    pub fn from_int(seed: u64) -> Self {
        let (lo, hi) = (seed as u32, (seed >> 32) as u32);
        let key = [lo, hi];
        Self::from_key(if hi != 0 { &key } else { &key[..1] })
    }

    /// `random.Random("text")`: the text's bytes followed by their SHA-512, read as one big
    /// number (Python's seed version 2).
    pub fn named(seed: &str) -> Self {
        let mut bytes = seed.as_bytes().to_vec();
        bytes.extend_from_slice(&sha512(seed.as_bytes()));
        let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
        let digits = &bytes[start..];
        // 32-bit words, the least significant first
        let mut key: Vec<u32> = digits.rchunks(4).map(|c| c.iter().fold(0u32, |v, &b| (v << 8) | b as u32)).collect();
        if key.is_empty() {
            key.push(0);
        }
        Self::from_key(&key)
    }

    /// The reference `init_by_array`.
    fn from_key(key: &[u32]) -> Self {
        let mut mt = [0u32; N];
        mt[0] = 19650218;
        for i in 1..N {
            mt[i] = 1812433253u32.wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_add(i as u32);
        }
        let (mut i, mut j) = (1, 0);
        for _ in 0..N.max(key.len()) {
            mt[i] = (mt[i] ^ (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(1664525)).wrapping_add(key[j]).wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                mt[0] = mt[N - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in 0..N - 1 {
            mt[i] = (mt[i] ^ (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(1566083941)).wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                mt[0] = mt[N - 1];
                i = 1;
            }
        }
        mt[0] = 0x8000_0000;
        PyRandom { mt, at: N }
    }

    fn next_u32(&mut self) -> u32 {
        if self.at >= N {
            for k in 0..N {
                let y = (self.mt[k] & 0x8000_0000) | (self.mt[(k + 1) % N] & 0x7fff_ffff);
                self.mt[k] = self.mt[(k + M) % N] ^ (y >> 1) ^ if y & 1 != 0 { 0x9908_b0df } else { 0 };
            }
            self.at = 0;
        }
        let mut y = self.mt[self.at];
        self.at += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// `rng.random()`: a float in [0, 1) with 53 bits.
    pub fn random(&mut self) -> f64 {
        let (a, b) = ((self.next_u32() >> 5) as f64, (self.next_u32() >> 6) as f64);
        (a * 67108864.0 + b) * (1.0 / 9007199254740992.0)
    }

    /// `rng.getrandbits(k)`, for k from 1 to 64.
    pub fn getrandbits(&mut self, k: u32) -> u64 {
        assert!((1..=64).contains(&k), "getrandbits: 1 to 64 bits");
        if k <= 32 {
            return (self.next_u32() >> (32 - k)) as u64;
        }
        let lo = self.next_u32() as u64;
        let hi = (self.next_u32() >> (64 - k)) as u64;
        (hi << 32) | lo
    }

    /// Python's `_randbelow(n)`: a whole number in [0, n), n at least 1.
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "below: n must be positive");
        let k = 64 - n.leading_zeros();
        loop {
            let r = self.getrandbits(k);
            if r < n {
                return r;
            }
        }
    }

    /// `rng.randrange(start, stop, step)`, step at least 1.
    pub fn randrange(&mut self, start: i64, stop: i64, step: i64) -> i64 {
        assert!(step >= 1 && stop > start, "randrange: empty range");
        let n = (stop - start + step - 1).div_euclid(step);
        start + step * self.below(n as u64) as i64
    }

    /// `rng.randint(a, b)`: both ends included.
    pub fn randint(&mut self, a: i64, b: i64) -> i64 {
        self.randrange(a, b + 1, 1)
    }

    /// `rng.choice(seq)`.
    pub fn choice<'a, T>(&mut self, seq: &'a [T]) -> &'a T {
        &seq[self.below(seq.len() as u64) as usize]
    }

    /// `rng.shuffle(xs)`.
    pub fn shuffle<T>(&mut self, xs: &mut [T]) {
        for i in (1..xs.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            xs.swap(i, j);
        }
    }

    /// `rng.uniform(a, b)`.
    pub fn uniform(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.random()
    }
}
