//! Scope context objects for captured-and-mutated bindings.
//!
//! A binding that a closure captures and someone mutates needs shared heap
//! storage. #11179 gave each such binding its own movable GC cell. Every cell
//! was then a separate GC root in the defining frame and a separate capture
//! slot in each closure, and under RS4GC statepoints the relocation work is
//! (#live GC pointers) × (#safepoints). A webpack-style module factory keeps
//! ~300 cells live across thousands of safepoints.
//!
//! This module groups those bindings into **scope objects** (V8 `Context`,
//! SpiderMonkey environment objects): one `GC_TYPE_SCOPE` cell per activation
//! of a group, with one NaN-boxed slot per binding. The defining frame keeps
//! ONE root per group and a capturing closure keeps ONE capture slot per group.
//!
//! # Where a group lives in the HIR
//!
//! A group is a `Stmt::PreallocateBoxes` / `Stmt::PreallocateTdzBoxes`
//! statement. [`group_scope_boxes`] (run by the driver after the async and
//! generator transforms) rewrites those statements and inserts new ones, and
//! codegen allocates ONE scope object per statement for the ids in it that
//! [`ScopeMap`] accepts. Reusing the existing statement keeps every existing
//! rule about preallocated bindings (a later `Stmt::Let` stores instead of
//! allocating, TDZ seeding, re-execution per block entry) intact.
//!
//! # Grouping rule
//!
//! Two bindings share an object only when
//! 1. their homes (first declaration in pre-order) are statements of the SAME
//!    statement list, so one allocation at the earliest home runs once per
//!    activation of that list. A loop body list is activated once per
//!    iteration, so a loop-body group is fresh per iteration (per-iteration
//!    `let`/`const` semantics);
//! 2. they are captured by exactly the SAME set of closures (every closure,
//!    at any nesting depth, whose body or capture list names the binding).
//!    A closure that outlives its siblings therefore retains only bindings it
//!    can itself name, never an unrelated binding a short-lived sibling
//!    captured (the V8 shared-context leak);
//! 3. they agree on TDZ seeding.
//!
//! Two refinements keep the root count down where splitting buys nothing:
//! a body's hoisted function declarations count as ONE capturing closure (they
//! are all created at scope entry and are the scope's long-lived API — the
//! module-factory shape), and a statement list whose bindings would still
//! split into more than 16 objects gets one object per TDZ kind (a bundler's
//! module-scale wrapper, whose closures live as long as the module anyway).
//! Arrows and function expressions each remain their own closure class, so a
//! surviving callback never retains a binding only a dead sibling captured.
//!
//! A binding is eligible only when its home dominates every reference and
//! every other declaration of it (they all sit in the home statement itself or
//! in later siblings of it), it appears in at most one preallocation statement
//! in the module, and it is declared by a `Stmt::Let` or a preallocation (not
//! a parameter, catch parameter or `for` head). Anything else keeps its own
//! per-binding cell, exactly as before.
//!
//! Nested scopes are not chained through parent pointers: a closure captures
//! each scope object it needs directly. That costs one capture slot per group
//! it names, and buys one dereference per access and no retention of an outer
//! scope a closure never names.

pub(crate) mod access;
mod analysis;
pub mod pass;

use std::collections::{HashMap, HashSet};

use perry_hir::{Expr, Module as HirModule, Stmt};

pub use pass::group_scope_boxes;

/// Upper bound on one scope object's slot count. Mirrors the runtime's
/// `SCOPE_MAX_SLOTS`; the pass splits larger groups.
pub(crate) const SCOPE_MAX_SLOTS: usize = 4096;

/// A binding's place in its scope object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScopeSlot {
    /// The group's representative id: its first member. The representative's
    /// frame slot is the group's root, and closures key the group's capture
    /// slot by it.
    pub rep: u32,
    /// Slot index inside the object.
    pub index: u32,
    /// Number of slots in the object.
    pub len: u32,
    /// The group is seeded with the TDZ sentinel (`PreallocateTdzBoxes`), so
    /// a read must check for it.
    pub tdz: bool,
}

/// Module-wide binding → scope-slot map. Local ids are module-unique, so one
/// flat map serves every function, closure and method body.
#[derive(Default, Debug)]
pub struct ScopeMap {
    slots: HashMap<u32, ScopeSlot>,
    members: HashMap<u32, Vec<u32>>,
}

impl ScopeMap {
    pub fn slot(&self, id: u32) -> Option<ScopeSlot> {
        self.slots.get(&id).copied()
    }

    /// The group's members in slot order (`rep` first). Empty for an id that
    /// is not a representative.
    pub fn members(&self, rep: u32) -> &[u32] {
        self.members.get(&rep).map_or(&[], Vec::as_slice)
    }

    /// The id a closure's capture layout uses for `id`: its group's
    /// representative, or `id` itself.
    pub fn capture_key(&self, id: u32) -> u32 {
        self.slots.get(&id).map_or(id, |slot| slot.rep)
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn group_count(&self) -> usize {
        self.members.len()
    }

    pub fn binding_count(&self) -> usize {
        self.slots.len()
    }

    /// Collapse a closure's capture id list onto its layout: every grouped id
    /// is replaced by its group's representative at the position of the
    /// group's first occurrence. Creation site and body both call this on the
    /// same list, so they agree on every index.
    pub fn collapse_captures(&self, ids: Vec<u32>) -> Vec<u32> {
        if self.slots.is_empty() {
            return ids;
        }
        let mut seen = HashSet::with_capacity(ids.len());
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let key = self.capture_key(id);
            if seen.insert(key) {
                out.push(key);
            }
        }
        out
    }

    /// Build the map from the module's preallocation statements. The
    /// statement is the group; its members are the ids codegen can prove
    /// dominated (see the module docs). Called after `module_globals` is known:
    /// a globalized binding already has shared storage and never joins.
    pub fn build(
        hir: &HirModule,
        module_boxed_vars: &HashSet<u32>,
        module_globals: &HashMap<u32, String>,
    ) -> Self {
        let mut map = ScopeMap::default();
        if module_boxed_vars.is_empty() {
            return map;
        }
        let counts = analysis::prealloc_counts(hir);
        for_each_body(hir, &mut |stmts: &[Stmt]| {
            let mut interest = HashSet::new();
            analysis::collect_shallow_prealloc_ids(stmts, &mut interest);
            interest.retain(|id| {
                counts.get(id) == Some(&1)
                    && module_boxed_vars.contains(id)
                    && !module_globals.contains_key(id)
            });
            if interest.is_empty() {
                return;
            }
            let facts = analysis::analyze_body(stmts, &interest);
            for prealloc in &facts.preallocs {
                let members: Vec<u32> = prealloc
                    .ids
                    .iter()
                    .copied()
                    .filter(|id| {
                        interest.contains(id)
                            && facts
                                .dominating_home(*id)
                                .is_some_and(|h| h.num == prealloc.num)
                    })
                    .take(SCOPE_MAX_SLOTS)
                    .collect();
                let Some(&rep) = members.first() else {
                    continue;
                };
                let len = members.len() as u32;
                for (index, id) in members.iter().enumerate() {
                    map.slots.insert(
                        *id,
                        ScopeSlot {
                            rep,
                            index: index as u32,
                            len,
                            tdz: prealloc.tdz,
                        },
                    );
                }
                map.members.insert(rep, members);
            }
        });
        map
    }
}

/// Drop the frame-root slots of non-representative group members (the group
/// root is the representative's slot) and renumber densely in slot order.
pub(crate) fn compact_root_slots(
    map: HashMap<u32, u32>,
    scope_map: &ScopeMap,
) -> HashMap<u32, u32> {
    if scope_map.is_empty() {
        return map;
    }
    let mut entries: Vec<(u32, u32)> = map
        .into_iter()
        .filter(|(id, _)| scope_map.slot(*id).is_none_or(|s| s.rep == *id))
        .collect();
    entries.sort_unstable_by_key(|(_, slot)| *slot);
    entries
        .into_iter()
        .enumerate()
        .map(|(i, (id, _))| (id, i as u32))
        .collect()
}

/// Visit every function-like body in the module: top-level functions, class
/// members, the module init, and every closure body nested anywhere in them.
pub(crate) fn for_each_body(hir: &HirModule, f: &mut dyn FnMut(&[Stmt])) {
    let mut roots: Vec<&[Stmt]> = vec![&hir.init];
    let mut root_exprs: Vec<&Expr> = Vec::new();
    for func in &hir.functions {
        roots.push(&func.body);
        push_param_defaults(&func.params, &mut root_exprs);
    }
    for c in &hir.classes {
        for m in c
            .methods
            .iter()
            .chain(c.static_methods.iter())
            .chain(c.getters.iter().map(|(_, g)| g))
            .chain(c.setters.iter().map(|(_, s)| s))
            .chain(c.computed_members.iter().map(|m| &m.function))
            .chain(c.constructor.iter())
        {
            roots.push(&m.body);
            push_param_defaults(&m.params, &mut root_exprs);
        }
        for field in c.fields.iter().chain(c.static_fields.iter()) {
            root_exprs.extend(field.init.iter());
            root_exprs.extend(field.key_expr.iter());
        }
        if let Some(e) = &c.extends_expr {
            root_exprs.push(e);
        }
    }
    for g in &hir.globals {
        root_exprs.extend(g.init.iter());
    }
    for stmts in roots {
        f(stmts);
        analysis::for_each_closure_in_stmts(stmts, &mut |body| for_each_body_in_closure(body, f));
    }
    for e in root_exprs {
        analysis::for_each_closure_in_expr(e, &mut |body| for_each_body_in_closure(body, f));
    }
}

fn for_each_body_in_closure(body: &[Stmt], f: &mut dyn FnMut(&[Stmt])) {
    f(body);
    analysis::for_each_closure_in_stmts(body, &mut |inner| for_each_body_in_closure(inner, f));
}

fn push_param_defaults<'a>(params: &'a [perry_hir::Param], out: &mut Vec<&'a Expr>) {
    out.extend(params.iter().filter_map(|p| p.default.as_ref()));
}

#[cfg(test)]
mod tests;
