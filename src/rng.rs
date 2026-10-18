//! Small, dependency-free randomness helpers.
//!
//! The bot needs two kinds of randomness: ordinary picks, and "daily fate"
//! picks that must stay the same for the same input on the same day. A seeded
//! SplitMix64 covers both without pulling in a full RNG crate.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::time::{SystemTime, UNIX_EPOCH};

/// A source of uniformly distributed 64-bit values.
pub trait RandomSource {
    fn next_u64(&mut self) -> u64;

    /// Returns an index in `0..len`.
    ///
    /// # Panics
    ///
    /// Panics if `len` is zero.
    fn below(&mut self, len: usize) -> usize {
        assert!(len > 0, "cannot pick from an empty range");
        // Lemire's multiply-shift: maps a 64-bit value onto 0..len.
        ((u128::from(self.next_u64()) * len as u128) >> 64) as usize
    }
}

/// SplitMix64 generator. Fast, tiny, and good enough for picking food.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Seeds from the per-process random hasher keys mixed with the clock.
    pub fn from_entropy() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        Self::new(RandomState::new().hash_one(nanos))
    }
}

impl RandomSource for SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Stable 64-bit FNV-1a hash over several byte strings.
///
/// Unlike `std`'s hasher, the output never changes between runs or Rust
/// versions, so it is safe to use for seeds that must be reproducible.
pub fn stable_hash(parts: &[&[u8]]) -> u64 {
    const OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01B3;

    let mut hash = OFFSET;
    for part in parts {
        for byte in part.iter() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
        // Separator so ["ab", "c"] and ["a", "bc"] hash differently.
        hash ^= 0xFF;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Picks one item uniformly. Returns `None` for an empty slice.
pub fn pick<'a, T>(rng: &mut impl RandomSource, items: &'a [T]) -> Option<&'a T> {
    if items.is_empty() {
        None
    } else {
        Some(&items[rng.below(items.len())])
    }
}

/// Picks one item according to integer weights. Zero-weight items are never
/// picked. Returns `None` when the slice is empty or every weight is zero.
pub fn pick_weighted<'a, T>(
    rng: &mut impl RandomSource,
    items: &'a [T],
    weight: impl Fn(&T) -> u32,
) -> Option<&'a T> {
    let total: u64 = items.iter().map(|item| u64::from(weight(item))).sum();
    if total == 0 {
        return None;
    }

    // `total` fits in usize on every supported target: at most u32::MAX per item.
    let mut ticket = rng.below(usize::try_from(total).ok()?) as u64;
    for item in items {
        let item_weight = u64::from(weight(item));
        if ticket < item_weight {
            return Some(item);
        }
        ticket -= item_weight;
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Returns a fixed sequence of values, cycling when exhausted.
    pub(crate) struct ScriptedRng {
        values: Vec<u64>,
        next: usize,
    }

    impl ScriptedRng {
        pub(crate) fn new(values: Vec<u64>) -> Self {
            Self { values, next: 0 }
        }

        /// Always yields zero, so every pick takes the first candidate.
        pub(crate) fn zeros() -> Self {
            Self::new(vec![0])
        }
    }

    impl RandomSource for ScriptedRng {
        fn next_u64(&mut self) -> u64 {
            let value = self.values[self.next % self.values.len()];
            self.next += 1;
            value
        }
    }

    #[test]
    fn splitmix_is_deterministic_for_a_seed() {
        let mut a = SplitMix64::new(42);
        let mut b = SplitMix64::new(42);
        let first: Vec<u64> = (0..4).map(|_| a.next_u64()).collect();
        let second: Vec<u64> = (0..4).map(|_| b.next_u64()).collect();
        assert_eq!(first, second);
        assert_ne!(first[0], first[1]);
    }

    #[test]
    fn below_stays_in_range_at_the_extremes() {
        assert_eq!(ScriptedRng::new(vec![0]).below(7), 0);
        assert_eq!(ScriptedRng::new(vec![u64::MAX]).below(7), 6);
        assert_eq!(ScriptedRng::new(vec![u64::MAX]).below(1), 0);
    }

    #[test]
    #[should_panic(expected = "empty range")]
    fn below_panics_on_empty_range() {
        ScriptedRng::zeros().below(0);
    }

    #[test]
    fn pick_handles_empty_and_non_empty() {
        let empty: [u8; 0] = [];
        assert_eq!(pick(&mut ScriptedRng::zeros(), &empty), None);
        assert_eq!(
            pick(&mut ScriptedRng::new(vec![u64::MAX]), &[1, 2, 3]),
            Some(&3)
        );
    }

    #[test]
    fn pick_weighted_respects_weights() {
        let items = [("never", 0), ("a", 1), ("b", 3)];
        let weight = |item: &(&str, u32)| item.1;
        assert_eq!(
            pick_weighted(&mut ScriptedRng::zeros(), &items, weight)
                .unwrap()
                .0,
            "a"
        );
        assert_eq!(
            pick_weighted(&mut ScriptedRng::new(vec![u64::MAX]), &items, weight)
                .unwrap()
                .0,
            "b"
        );
    }

    #[test]
    fn pick_weighted_returns_none_without_weight() {
        let items = [("a", 0), ("b", 0)];
        assert!(pick_weighted(&mut ScriptedRng::zeros(), &items, |item| item.1).is_none());
        let empty: [(&str, u32); 0] = [];
        assert!(pick_weighted(&mut ScriptedRng::zeros(), &empty, |item| item.1).is_none());
    }

    #[test]
    fn stable_hash_is_stable_and_separates_parts() {
        assert_eq!(stable_hash(&[b"abc"]), stable_hash(&[b"abc"]));
        assert_ne!(stable_hash(&[b"ab", b"c"]), stable_hash(&[b"a", b"bc"]));
        assert_ne!(stable_hash(&[]), stable_hash(&[b""]));
    }
}
