//! Points-to sets: a sorted small vector that switches to a hash set above a
//! threshold, plus a tiny Fx-style hasher (no external crate).

use std::collections::HashSet;
use std::hash::{BuildHasherDefault, Hasher};

/// Fx hash (rustc's): fast and deterministic. Iteration order of the maps and
/// sets that use it is never observable in output: every dump sorts.
#[derive(Default, Clone, Copy)]
pub struct FxHasher {
    hash: u64,
}

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.add(b as u64);
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

impl FxHasher {
    #[inline]
    fn add(&mut self, i: u64) {
        self.hash = (self.hash.rotate_left(5) ^ i).wrapping_mul(SEED);
    }
}

pub type FxBuild = BuildHasherDefault<FxHasher>;
pub type FxHashMap<K, V> = std::collections::HashMap<K, V, FxBuild>;
pub type FxHashSet<K> = HashSet<K, FxBuild>;

const SMALL_MAX: usize = 48;

/// A set of token ids.
#[derive(Clone, Debug)]
pub enum Pts {
    Small(Vec<u32>),
    Big(FxHashSet<u32>),
}

impl Default for Pts {
    fn default() -> Self {
        Pts::Small(Vec::new())
    }
}

impl Pts {
    pub fn contains_tok(&self, t: u32) -> bool {
        match self {
            Pts::Small(v) => v.binary_search(&t).is_ok(),
            Pts::Big(s) => s.contains(&t),
        }
    }

    /// Insert; true when new.
    #[inline]
    pub(crate) fn insert(&mut self, t: u32) -> bool {
        match self {
            Pts::Small(v) => match v.binary_search(&t) {
                Ok(_) => false,
                Err(at) => {
                    v.insert(at, t);
                    if v.len() > SMALL_MAX {
                        let s: FxHashSet<u32> = v.iter().copied().collect();
                        *self = Pts::Big(s);
                    }
                    true
                }
            },
            Pts::Big(s) => s.insert(t),
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Pts::Small(v) => v.len(),
            Pts::Big(s) => s.len(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Sorted copy of the members.
    pub(crate) fn to_sorted(&self) -> Vec<u32> {
        match self {
            Pts::Small(v) => v.clone(),
            Pts::Big(s) => {
                let mut v: Vec<u32> = s.iter().copied().collect();
                v.sort_unstable();
                v
            }
        }
    }
}
