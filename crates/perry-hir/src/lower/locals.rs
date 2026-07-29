//! Scope-local bindings with an O(1) name→position index (#5267).
//!
//! The lowerer resolves every identifier reference by finding the
//! innermost (most-recently-pushed) binding with a given name. The old
//! representation was a bare `Vec<(String, LocalId, Type)>` scanned with a
//! reverse `find`/`rposition` per reference, so a scope holding N bindings
//! with N references lowered in **O(n²)** time. Real minified/bundled JS
//! puts tens of thousands of bindings and references in one module (or one
//! wrapper-function) scope, so `check-lower` stalled for minutes and got
//! killed with no diagnostic on large bundles.
//!
//! `Locals` keeps the same ordered stack (`entries`, the authoritative
//! source of truth) plus a side `index` mapping each name to the ascending
//! list of positions in `entries` that currently hold that name. The *last*
//! position in a name's list is the innermost binding, so `lookup`,
//! `lookup_index`, and `lookup_type` are O(1).
//!
//! Every operation that moves a binding's position (`push`, `drain_from`,
//! `extend`, `remove`) goes through an inherent method here that keeps
//! `index` in sync. Read-only and in-place ops (`iter`, `iter_mut`,
//! slicing, `len`) reach the underlying slice via `Deref`/`DerefMut` —
//! those never move a binding or rename it, so the index stays valid. Note
//! `DerefMut` hands out `&mut [..]` (a slice), **not** `&mut Vec`, so
//! callers cannot bypass the index with `Vec::push`/`remove`/`truncate`.

use std::ops::{Deref, DerefMut};

use crate::types::{LocalId, Type};
use std::collections::{HashMap, HashSet};

/// Ordered stack of scope-local bindings with a name→positions index.
#[derive(Debug, Clone, Default)]
pub(crate) struct Locals {
    /// Authoritative ordered list of `(name, id, type)` bindings.
    entries: Vec<(String, LocalId, Type)>,
    /// name -> ascending positions into `entries`. The last entry of each
    /// list is the innermost (most-recent) binding for that name.
    index: HashMap<String, Vec<usize>>,
    /// Set of the `LocalId`s currently present in `entries`. Maintained in
    /// lockstep with `entries` by every mutating method, so closure-capture
    /// analysis can do O(1) "is this id an enclosing local?" membership tests
    /// against the *current* scope without rebuilding a `HashSet` from the
    /// `entries` slice on every closure. Each binding gets a fresh monotonic
    /// `LocalId` (`fresh_local`), so ids are unique within `entries` and a
    /// plain set (no refcount) stays in sync. See `compute_closure_captures`
    /// and `nested_fn_decl` — both formerly rebuilt this set per closure,
    /// making capture analysis O(scope) per closure = O(n²) over a scope of
    /// n sibling closures.
    id_set: HashSet<LocalId>,
}

impl Locals {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Push a new innermost binding. O(1) amortized.
    pub(crate) fn push(&mut self, entry: (String, LocalId, Type)) {
        let pos = self.entries.len();
        self.index.entry(entry.0.clone()).or_default().push(pos);
        self.id_set.insert(entry.1);
        self.entries.push(entry);
    }

    /// The set of `LocalId`s currently in scope. O(1). Used by closure-capture
    /// analysis as the "enclosing locals" membership view, replacing a
    /// per-closure rebuild from the `entries` slice (#5267 follow-up).
    pub(crate) fn id_set(&self) -> &HashSet<LocalId> {
        &self.id_set
    }

    /// Position of the innermost binding named `name`, if any. O(1).
    /// Equivalent to the old `iter().rposition(|(n, ..)| n == name)`.
    pub(crate) fn lookup_index(&self, name: &str) -> Option<usize> {
        self.index.get(name).and_then(|v| v.last()).copied()
    }

    /// `LocalId` of the innermost binding named `name`, if any. O(1).
    pub(crate) fn lookup(&self, name: &str) -> Option<LocalId> {
        self.lookup_index(name).map(|i| self.entries[i].1)
    }

    /// `Type` of the innermost binding named `name`, if any. O(1).
    pub(crate) fn lookup_type(&self, name: &str) -> Option<&Type> {
        self.lookup_index(name).map(|i| &self.entries[i].2)
    }

    /// Mutable `Type` of the innermost binding named `name`, if any. O(1).
    /// Replaces the old `iter_mut().rev().find(|(n, ..)| n == name)` type
    /// patch-ups that were O(n) per declaration (#5267).
    pub(crate) fn lookup_type_mut(&mut self, name: &str) -> Option<&mut Type> {
        let idx = self.lookup_index(name)?;
        Some(&mut self.entries[idx].2)
    }

    /// Position of the innermost binding named `name` whose position is at
    /// or past `min_pos` (i.e. introduced in the current scope), if any.
    /// O(1): the innermost binding for a name has the maximal position, so
    /// one exists at `>= min_pos` iff that maximum is `>= min_pos`. Replaces
    /// the `iter().enumerate().rev().any(|(idx, (n, ..))| n == name && idx >=
    /// min_pos)` scans that were O(n) per declaration (#5267).
    pub(crate) fn lookup_index_in_scope(&self, name: &str, min_pos: usize) -> Option<usize> {
        self.lookup_index(name).filter(|&pos| pos >= min_pos)
    }

    /// Iterate the bindings named `name` from innermost to outermost
    /// (descending position), yielding `(position, &binding)`. O(number of
    /// bindings sharing `name`) — a single step for the common case of a
    /// unique name — instead of the old O(n) reverse scan of the entire
    /// stack (#5267). Used by the `var`-redeclaration reuse checks, which
    /// need the innermost binding matching an extra predicate (e.g.
    /// var-hoisted) plus its position for an O(1) type patch-up afterwards.
    pub(crate) fn iter_named<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = (usize, &'a (String, LocalId, Type))> + 'a {
        self.index
            .get(name)
            .into_iter()
            .flat_map(move |positions| positions.iter().rev().map(move |&p| (p, &self.entries[p])))
    }

    /// Mutable `Type` of the binding at `pos`. Pairs with `iter_named` /
    /// `lookup_index*` to patch a binding's inferred type without a second
    /// O(n) scan to re-find it (#5267).
    pub(crate) fn type_mut_at(&mut self, pos: usize) -> &mut Type {
        &mut self.entries[pos].2
    }

    /// Drain every binding at position `>= mark` (a scope/block pop),
    /// returning the drained entries in ascending-position order. The caller
    /// may filter and `extend` survivors back (var-hoisted / sloppy globals).
    pub(crate) fn drain_from(&mut self, mark: usize) -> Vec<(String, LocalId, Type)> {
        if mark >= self.entries.len() {
            return Vec::new();
        }
        let drained: Vec<(String, LocalId, Type)> = self.entries.drain(mark..).collect();
        // Each drained entry sat at a position `>= mark`, and for any single
        // name those positions are the highest in its index list. Walking the
        // drained entries in reverse (descending positions) lets us pop them
        // off the back one-for-one, leaving any `< mark` positions intact.
        for (name, _, _) in drained.iter().rev() {
            let now_empty = match self.index.get_mut(name) {
                Some(positions) => {
                    positions.pop();
                    positions.is_empty()
                }
                None => false,
            };
            if now_empty {
                self.index.remove(name);
            }
        }
        for (_, id, _) in &drained {
            self.id_set.remove(id);
        }
        drained
    }

    /// Re-append previously-drained survivors, assigning fresh (compacted)
    /// positions. Mirrors `Vec::extend`; keeps the index in sync.
    pub(crate) fn extend<I: IntoIterator<Item = (String, LocalId, Type)>>(&mut self, iter: I) {
        for entry in iter {
            self.push(entry);
        }
    }

    /// Remove the binding at `idx`, shifting later bindings down by one.
    /// Rare (native-require dedup, #5216), so a full O(n) reindex is fine.
    pub(crate) fn remove(&mut self, idx: usize) -> (String, LocalId, Type) {
        let removed = self.entries.remove(idx);
        self.reindex();
        removed
    }

    /// Rebuild the whole name→positions index and the id set from `entries`.
    fn reindex(&mut self) {
        self.index.clear();
        self.id_set.clear();
        for (pos, (name, id, _)) in self.entries.iter().enumerate() {
            self.index.entry(name.clone()).or_default().push(pos);
            self.id_set.insert(*id);
        }
    }
}

impl Deref for Locals {
    type Target = [(String, LocalId, Type)];

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

impl DerefMut for Locals {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.entries
    }
}
