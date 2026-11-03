//! Points-to sets: a sorted small vector that switches to a hash set above a
//! threshold. The Fx hasher lives in `perry_hir::fx`.

// Iteration order of the maps and sets that use it is never observable in
// output: every dump sorts.
pub use perry_hir::fx::{FxBuild, FxHashMap, FxHashSet, FxHasher};

use std::sync::Arc;

const SMALL_MAX: usize = 48;

/// A set of token ids.
#[derive(Clone, Debug)]
pub enum Pts {
    Small(Vec<u32>),
    Shared(Arc<Vec<u32>>),
    Big(Arc<FxHashSet<u32>>),
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
            Pts::Shared(v) => v.binary_search(&t).is_ok(),
            Pts::Big(s) => s.contains(&t),
        }
    }

    /// Identity of immutable set storage proves equality.
    pub(crate) fn covered_by(&self, other: &Self) -> bool {
        if self.len() == 1 {
            return other.contains_tok(self.to_sorted()[0]);
        }
        match (self, other) {
            (Pts::Big(a), Pts::Big(b)) => Arc::ptr_eq(a, b),
            (Pts::Shared(a), Pts::Shared(b)) => Arc::ptr_eq(a, b),
            (Pts::Small(a), Pts::Small(b)) => a.len() == 1 && b.len() == 1 && a[0] == b[0],
            _ => false,
        }
    }

    pub(crate) fn shared_owners(&self) -> usize {
        match self {
            Pts::Big(s) => Arc::strong_count(s),
            Pts::Shared(s) => Arc::strong_count(s),
            _ => 0,
        }
    }

    pub(crate) fn shared_clone(&mut self) -> Self {
        if let Pts::Small(v) = self {
            *self = Pts::Shared(Arc::new(std::mem::take(v)));
        }
        self.clone()
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
                        *self = Pts::Big(Arc::new(s));
                    }
                    true
                }
            },
            Pts::Shared(v) => {
                let Err(at) = v.binary_search(&t) else {
                    return false;
                };
                let values = Arc::make_mut(v);
                values.insert(at, t);
                if values.len() > SMALL_MAX {
                    *self = Pts::Big(Arc::new(values.iter().copied().collect()));
                }
                true
            }
            Pts::Big(s) => {
                if s.contains(&t) {
                    false
                } else {
                    Arc::make_mut(s).insert(t)
                }
            }
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Pts::Small(v) => v.len(),
            Pts::Shared(v) => v.len(),
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
            Pts::Shared(v) => out.extend_from_slice(v),
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
            Pts::Shared(v) => v.as_ref().clone(),
            Pts::Big(s) => {
                let mut v: Vec<u32> = s.iter().copied().collect();
                v.sort_unstable();
                v
            }
        }
    }
}
