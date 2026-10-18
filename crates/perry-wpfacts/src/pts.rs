//! Points-to sets: a sorted small vector that switches to a hash set above a
//! threshold. The Fx hasher lives in `perry_hir::fx`.

// Iteration order of the maps and sets that use it is never observable in
// output: every dump sorts.
pub use perry_hir::fx::{FxBuild, FxHashMap, FxHashSet, FxHasher};

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

    /// Append the members, sorted, to `out` (a reused buffer).
    pub(crate) fn sorted_into(&self, out: &mut Vec<u32>) {
        match self {
            Pts::Small(v) => out.extend_from_slice(v),
            Pts::Big(s) => {
                let at = out.len();
                out.extend(s.iter().copied());
                out[at..].sort_unstable();
            }
        }
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
