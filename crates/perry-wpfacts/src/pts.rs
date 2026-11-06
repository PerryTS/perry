//! Points-to sets. A set of two or more members is shared storage; during a
//! solve the solver interns it in a `SetTable`, so equal sets are one
//! allocation and a set already held by a successor is found by a pointer
//! test. The Fx hasher lives in `perry_hir::fx`.

// Iteration order of the maps and sets that use it is never observable in
// output: every dump sorts.
pub use perry_hir::fx::{FxBuild, FxHashMap, FxHashSet, FxHasher};

use std::cell::Cell;
use std::rc::Rc;

const SMALL_MAX: usize = 48;

/// A set of token ids: none or one member by value, more as shared storage.
#[derive(Clone, Debug)]
pub enum Pts {
    Small(Vec<u32>),
    Set(Rc<SetData>),
}

impl Default for Pts {
    fn default() -> Self {
        Pts::Small(Vec::new())
    }
}

/// The members of a set of two or more, with an order-free content hash.
#[derive(Debug)]
pub struct SetData {
    hash: u64,
    /// Held by the solve's `SetTable` (the only interned set with this
    /// content).
    interned: Cell<bool>,
    items: Items,
}

#[derive(Clone, Debug)]
enum Items {
    Sorted(Vec<u32>),
    Hashed(FxHashSet<u32>),
}

/// A copy is never interned: only the table's own entry is.
impl Clone for SetData {
    fn clone(&self) -> Self {
        SetData {
            hash: self.hash,
            interned: Cell::new(false),
            items: self.items.clone(),
        }
    }
}

/// One member's share of the content hash (a sum, so the order of arrival
/// does not matter).
fn mix(t: u32) -> u64 {
    let mut z = (t as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl SetData {
    fn from_sorted(v: Vec<u32>) -> Self {
        let hash = v.iter().fold(0u64, |h, &t| h.wrapping_add(mix(t)));
        let items = if v.len() > SMALL_MAX {
            Items::Hashed(v.into_iter().collect())
        } else {
            Items::Sorted(v)
        };
        SetData {
            hash,
            interned: Cell::new(false),
            items,
        }
    }

    fn len(&self) -> usize {
        match &self.items {
            Items::Sorted(v) => v.len(),
            Items::Hashed(s) => s.len(),
        }
    }

    fn contains(&self, t: u32) -> bool {
        match &self.items {
            Items::Sorted(v) => v.binary_search(&t).is_ok(),
            Items::Hashed(s) => s.contains(&t),
        }
    }

    fn insert(&mut self, t: u32) -> bool {
        let new = match &mut self.items {
            Items::Sorted(v) => match v.binary_search(&t) {
                Ok(_) => false,
                Err(at) => {
                    v.insert(at, t);
                    true
                }
            },
            Items::Hashed(s) => s.insert(t),
        };
        if new {
            self.hash = self.hash.wrapping_add(mix(t));
            if let Items::Sorted(v) = &mut self.items {
                if v.len() > SMALL_MAX {
                    self.items = Items::Hashed(std::mem::take(v).into_iter().collect());
                }
            }
        }
        new
    }

    fn all_in(&self, other: &SetData) -> bool {
        match &self.items {
            Items::Sorted(v) => v.iter().all(|&t| other.contains(t)),
            Items::Hashed(s) => s.iter().all(|&t| other.contains(t)),
        }
    }
}

impl Pts {
    pub fn contains_tok(&self, t: u32) -> bool {
        match self {
            Pts::Small(v) => v.contains(&t),
            Pts::Set(s) => s.contains(t),
        }
    }

    /// `self` ⊆ `other`, decided without a scan: a singleton contained in
    /// `other`, or the same interned storage (equal content).
    pub(crate) fn covered_by(&self, other: &Self) -> bool {
        match (self, other) {
            (Pts::Small(a), _) => a.len() == 1 && other.contains_tok(a[0]),
            (Pts::Set(a), Pts::Set(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// Both hold the same storage (equal content).
    pub(crate) fn same_storage(&self, other: &Self) -> bool {
        matches!((self, other), (Pts::Set(a), Pts::Set(b)) if Rc::ptr_eq(a, b))
    }

    /// The address of shared storage (statistics: equal addresses are one
    /// stored set).
    pub(crate) fn storage_id(&self) -> Option<usize> {
        match self {
            Pts::Small(_) => None,
            Pts::Set(s) => Some(Rc::as_ptr(s) as usize),
        }
    }

    /// Stored as a hash set (more than `SMALL_MAX` members).
    pub(crate) fn is_hashed(&self) -> bool {
        matches!(self, Pts::Set(s) if matches!(s.items, Items::Hashed(_)))
    }

    /// Storage-level insert outside a solve (no interning; copies shared
    /// storage first). True when new.
    #[cfg(test)]
    pub(crate) fn insert(&mut self, t: u32) -> bool {
        match self {
            Pts::Small(v) => {
                if v.contains(&t) {
                    return false;
                }
                if v.is_empty() {
                    v.push(t);
                } else {
                    let mut all = vec![v[0], t];
                    all.sort_unstable();
                    *self = Pts::Set(Rc::new(SetData::from_sorted(all)));
                }
                true
            }
            Pts::Set(s) => !s.contains(t) && Rc::make_mut(s).insert(t),
        }
    }

    /// A handle on the same storage.
    #[cfg(test)]
    pub(crate) fn shared_clone(&mut self) -> Self {
        self.clone()
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Pts::Small(v) => v.len(),
            Pts::Set(s) => s.len(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Append the members, sorted, to `out` (a reused buffer).
    pub(crate) fn sorted_into(&self, out: &mut Vec<u32>) {
        match self {
            Pts::Small(v) => out.extend_from_slice(v),
            Pts::Set(s) => match &s.items {
                Items::Sorted(v) => out.extend_from_slice(v),
                Items::Hashed(h) => {
                    let at = out.len();
                    out.extend(h.iter().copied());
                    out[at..].sort_unstable();
                }
            },
        }
    }

    /// Sorted copy of the members.
    pub(crate) fn to_sorted(&self) -> Vec<u32> {
        let mut v = Vec::with_capacity(self.len());
        self.sorted_into(&mut v);
        v
    }
}

/// One solve's interned sets: content hash -> the one interned set of that
/// content (`more` holds the rare second set of a hash). It holds only sets
/// some node holds now (`release`), is the storage's index and not a cache
/// of results, and is cleared when the solve ends (`clear`).
#[derive(Default)]
pub struct SetTable {
    first: FxHashMap<u64, Rc<SetData>>,
    more: FxHashMap<u64, Vec<Rc<SetData>>>,
    /// Members copied to make a set that no other node held yet.
    pub copies: u64,
}

impl SetTable {
    fn find(&self, hash: u64, len: usize, eq: impl Fn(&SetData) -> bool) -> Option<Rc<SetData>> {
        let hit = |s: &&Rc<SetData>| s.len() == len && eq(s);
        if let Some(s) = self.first.get(&hash).filter(hit) {
            return Some(s.clone());
        }
        self.more.get(&hash)?.iter().find(hit).cloned()
    }

    fn insert(&mut self, s: &Rc<SetData>) {
        s.interned.set(true);
        match self.first.entry(s.hash) {
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(s.clone());
            }
            std::collections::hash_map::Entry::Occupied(_) => {
                self.more.entry(s.hash).or_default().push(s.clone());
            }
        }
    }

    fn remove(&mut self, s: &Rc<SetData>) {
        s.interned.set(false);
        let h = s.hash;
        if self.first.get(&h).is_some_and(|f| Rc::ptr_eq(f, s)) {
            match self.more.get_mut(&h).and_then(|v| v.pop()) {
                Some(next) => {
                    self.first.insert(h, next);
                }
                None => {
                    self.first.remove(&h);
                }
            }
        } else if let Some(v) = self.more.get_mut(&h) {
            v.retain(|o| !Rc::ptr_eq(o, s));
        }
        if self.more.get(&h).is_some_and(|v| v.is_empty()) {
            self.more.remove(&h);
        }
    }

    /// The interned set equal to `p` (taking it frees `p`'s storage), or
    /// `p` itself, interned.
    pub(crate) fn intern(&mut self, p: Pts) -> Pts {
        let Pts::Set(s) = p else { return p };
        if s.interned.get() {
            return Pts::Set(s);
        }
        if let Some(found) = self.find(s.hash, s.len(), |o| s.all_in(o)) {
            return Pts::Set(found);
        }
        self.insert(&s);
        Pts::Set(s)
    }

    /// A node no longer holds `p`: a set only the table still holds leaves
    /// it (and its storage is freed).
    pub(crate) fn release(&mut self, p: Pts) {
        if let Pts::Set(s) = &p {
            if s.interned.get() && Rc::strong_count(s) == 2 {
                self.remove(s);
            }
        }
    }

    /// `old` ∪ `new`, interned, where `new` holds distinct members not in
    /// `old`. Storage this node holds alone grows in place; a shared set is
    /// never changed: the equal interned set is taken when there is one,
    /// else the set is copied once.
    pub(crate) fn extend(&mut self, old: Pts, new: &[u32]) -> Pts {
        if new.is_empty() {
            return old;
        }
        let len = old.len() + new.len();
        let hash = new
            .iter()
            .fold(0u64, |h, &t| h.wrapping_add(mix(t)));
        match old {
            Pts::Small(v) => {
                if len == 1 {
                    return Pts::Small(new.to_vec());
                }
                let hash = v.iter().fold(hash, |h, &t| h.wrapping_add(mix(t)));
                let eq = |o: &SetData| v.iter().chain(new).all(|&t| o.contains(t));
                if let Some(found) = self.find(hash, len, eq) {
                    return Pts::Set(found);
                }
                let mut all = v;
                all.extend_from_slice(new);
                all.sort_unstable();
                let s = Rc::new(SetData::from_sorted(all));
                self.insert(&s);
                Pts::Set(s)
            }
            Pts::Set(mut s) => {
                if Rc::strong_count(&s) == 1 + s.interned.get() as usize {
                    if s.interned.get() {
                        self.remove(&s);
                    }
                    let data = Rc::get_mut(&mut s).expect("storage held by this node alone");
                    for &t in new {
                        data.insert(t);
                    }
                    return self.intern(Pts::Set(s));
                }
                let hash = s.hash.wrapping_add(hash);
                let eq = |o: &SetData| new.iter().all(|&t| o.contains(t)) && s.all_in(o);
                let next = match self.find(hash, len, eq) {
                    Some(found) => found,
                    None => {
                        self.copies += s.len() as u64;
                        let mut data = (*s).clone();
                        for &t in new {
                            data.insert(t);
                        }
                        let made = Rc::new(data);
                        self.insert(&made);
                        made
                    }
                };
                self.release(Pts::Set(s));
                Pts::Set(next)
            }
        }
    }

    /// The solve ended: the sets stay with their nodes, the index goes.
    pub(crate) fn clear(&mut self) {
        for s in self.first.values().chain(self.more.values().flatten()) {
            s.interned.set(false);
        }
        self.first = Default::default();
        self.more = Default::default();
    }
}
