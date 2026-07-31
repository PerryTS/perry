//! Representation-selection Phase 3b, **#7034 §4: return-shape facts**.
//!
//! ## What this proves, and why it is the cheapest of the three escapes
//!
//! `#7034` measured `Ptr<Shape>` promotion on `benchmarks/app-patterns/
//! kernels/batch.ts` — the object/property-heavy workload the representation
//! exists for — and found **zero** promoted locals. Every real record escapes
//! its producing scope, and `collectors/ptr_shape.rs` rule 2 listed `return`,
//! `call argument` and `array/object element` as outright disqualifiers, so
//! the proof died at the first escape.
//!
//! Returns are the escape that needs no cross-function agreement, no ABI
//! change, and no clone-and-route machinery at all (contrast `proven_this.rs`,
//! whose `this` proof emits an internal per-method clone and routes call sites
//! to it). Two independent halves:
//!
//! 1. **Producer side** (`ptr_shape.rs`, rule 2's return exemption). A local
//!    whose every other use is contained is promoted even though it is
//!    returned, because a `return` is a terminator: the caller cannot observe
//!    — let alone reshape — the object until after every access this pass
//!    licenses has already run.
//! 2. **Caller side** (this file). If a module function's every return path
//!    hands back a *freshly allocated, unaliased* object of one class, a call
//!    to it is rule-1 provenance of exactly `new C(...)` strength, so
//!    `const r = producer(...)` becomes an ordinary `Ptr<Shape>` candidate
//!    and every `r.field` in the caller lowers guard-free.
//!
//! ## The freshness obligation, and how it is discharged
//!
//! Rule 1's `new C(...)` seed carries two facts: the dynamic class is exactly
//! `C`, and — because the allocation is *right there* — no other reference to
//! the object exists yet. A call site sees neither for free: `function get() {
//! return CACHE; }` also "returns a `C`", but its result is aliased by the
//! module, so a caller-side shape proof over it would be unsound the moment
//! anything else reshapes `CACHE`.
//!
//! So a return-shape fact is only issued when **every** return of the producer
//! is one of:
//!
//! * `return new C(...)` — fresh by construction. (Object literals lower to
//!   `Expr::New { class_name: "__AnonShape_…" }`, so `return { k: v }` is this
//!   case too.) The constructor's `this`-flow safety is re-proven by the
//!   *caller's* own rule 3 walk over `C`, which runs on the seeded candidate
//!   exactly as it does for a literal `new`.
//! * `return <local>` where that local is itself a **fully proven Phase 3b
//!   local of the producer's body**. That is the whole containment proof —
//!   single `Let` with a `new C(...)` init, every use a declared-chain field
//!   access or a `this`-flow-vetted method call, no alias, no capture, no
//!   other escape — which is precisely "no reference to this object exists
//!   anywhere but the returned one". It is discharged by re-running
//!   `collect_shape_proven_ptr_locals` over the producer's body rather than
//!   by a second, weaker approximation of it.
//!
//! Anything else — `return CACHE`, `return this.field`, `return mk()`,
//! `return cond ? a : b` — yields no fact.
//!
//! ## Why the producer must not fall off its end
//!
//! `function f(x) { if (x) return new C(); }` returns `undefined` on the other
//! path. A caller that treated the result as a proven `C` would emit a bare
//! fixed-offset load against a NaN-boxed `undefined`. The fact therefore also
//! requires that control cannot reach the end of the body: the last statement
//! must itself be a value-returning `return`, or a `throw`.
//!
//! ## GC contract
//!
//! **Nothing here introduces a new site that holds an object pointer.** The
//! caller-side binding is an ordinary NaN-boxed local slot — the same storage
//! it had before this pass existed — shadow-bound by
//! `collect_pointer_typed_locals` / `js_shadow_slot_bind` like any other
//! object local, and the returned register is dead the moment the `Let` stores
//! it. Every access re-derives the raw pointer from that slot inside one
//! region (`ptr_shape.rs`'s tagged-at-rest contract), so an evacuating
//! scavenge that moves the object rewrites the bound slot and the next access
//! observes the new address. `TaPtr`'s callee-side no-bind shortcut is NOT
//! copied here — it is sound only for non-movable typed-array storage, and
//! `GC_TYPE_OBJECT` is movable (#6990, #7019).
//!
//! ## Numeric fields are deliberately not claimed
//!
//! See the `'cand` loop in `ptr_shape.rs`: the producer's own stores are not
//! in the caller's region, so no exhaustive-reachable-store proof is
//! available. Same stand-down, same reason, as `proven_this.rs`.
//!
//! Gated by `PERRY_PTR_SHAPE_LOCALS` along with the rest of Phase 3b — no new
//! env knob, so there is no new unexercised off-state.

use std::collections::{HashMap, HashSet};

use perry_hir::{Class, Expr, Function, Module, Stmt};

use super::ptr_shape::{chain_admissible, ptr_shape_locals_enabled};
use super::ptr_shape_report as report;
use super::ModuleDispatchFacts;

/// Module pre-pass: which module-level functions carry a return-shape fact.
///
/// `facts` must already have its barrier flags final and its own
/// `return_shape_functions` map still EMPTY — the per-producer proof re-enters
/// [`super::ptr_shape::collect_shape_proven_ptr_locals`], which consults that
/// map, and an empty map is what makes the recursion impossible.
pub(crate) fn collect_return_shape_functions(
    facts: &ModuleDispatchFacts,
    hir: &Module,
) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    if !ptr_shape_locals_enabled() || facts.has_shape_barrier_sites() {
        return out;
    }
    let classes: HashMap<String, &Class> = hir
        .classes
        .iter()
        .map(|c| (c.name.clone(), c))
        .collect::<HashMap<_, _>>();
    for f in &hir.functions {
        if let Some(class_name) = producer_return_class(f, &classes, facts) {
            out.insert(f.id, class_name);
        }
    }
    out
}

/// The class a call to `f` provably returns, or `None`.
fn producer_return_class(
    f: &Function,
    classes: &HashMap<String, &Class>,
    facts: &ModuleDispatchFacts,
) -> Option<String> {
    // Context restrictions, identical to every other Phase 1/3a/3b analysis:
    // the async-to-generator transform boxes body locals into one shared
    // mutable cell, so no containment fact survives it. A generator's `return`
    // is also not a single-exit terminator in the sense rule 2 relies on.
    if f.is_async || f.is_generator || f.was_plain_async {
        return None;
    }
    // GC: the caller's binding must be able to GET a shadow slot.
    // `collect_pointer_typed_locals` drops the slot for a local it can prove
    // non-pointer, and it proves that from the INIT expression's type — which
    // for a call is the callee's declared return type. A producer annotated
    // `: number` that actually returns an object would leave the caller with
    // a promoted `Ptr<Shape>` local in an UNROOTED alloca; an evacuating minor
    // would then move the object without rewriting the slot (#7019 ships that
    // default-on). Perry does not check annotations, so refuse the fact rather
    // than trust one. `Any`/`Unknown`/named/object types all keep the slot.
    //
    // Calls `pointer_locals`'s own predicate rather than restating it: a second
    // copy drifting by one `Type` variant is precisely how a value ends up
    // unrooted there while this pass treats it as a live movable pointer.
    if super::pointer_locals::is_definitely_non_pointer_type(&f.return_type) {
        return None;
    }
    // The body must not be able to fall off its end (module doc).
    match f.body.last() {
        Some(Stmt::Return(Some(_))) | Some(Stmt::Throw(_)) => {}
        _ => return None,
    }
    let mut returns = Vec::new();
    if !collect_own_returns(&f.body, &mut returns) {
        // A bare `return;` — the caller would see `undefined`.
        return None;
    }
    if returns.is_empty() {
        return None;
    }

    // Every return must agree on one class, and each must be a fresh form.
    let mut class_name: Option<&str> = None;
    let mut needs_body_proof: Vec<u32> = Vec::new();
    for r in &returns {
        let (name, local) = match r {
            Expr::New { class_name: c, .. } => (c.as_str(), None),
            Expr::LocalGet(id) => {
                // Resolved against the producer's own Phase 3b proof below;
                // find its declared class first so disagreement short-circuits.
                let c = seeded_class_of_local(&f.body, *id)?;
                (c, Some(*id))
            }
            _ => return None,
        };
        match class_name {
            None => class_name = Some(name),
            Some(prev) if prev == name => {}
            Some(_) => return None,
        }
        if let Some(id) = local {
            needs_body_proof.push(id);
        }
    }
    let class_name = class_name?;
    // Cheap rejections before the (relatively expensive) body proof: the class
    // must be one this module declares and Phase 3b admits at all.
    if !classes.contains_key(class_name) || !chain_admissible(classes, class_name) {
        return None;
    }

    if !needs_body_proof.is_empty() {
        // Discharge freshness by re-running the FULL Phase 3b proof over the
        // producer's body: a local it promotes has, by rule 2, no alias
        // anywhere, so the returned reference is the only one in existence.
        //
        // `module_globals` is empty on purpose and is not an approximation:
        // `codegen/module_globals_emit.rs` only ever records ids of top-level
        // `hir.init` lets, and every candidate here comes from a `Stmt::Let`
        // inside this function body.
        let boxed = crate::boxed_vars::collect_boxed_vars(&f.body);
        let _quiet = report::SuppressScope::new();
        let promoted = super::ptr_shape::collect_shape_proven_ptr_locals(
            &f.body,
            &boxed,
            &HashMap::new(),
            classes,
            facts,
            &HashSet::new(),
        );
        for id in &needs_body_proof {
            match promoted.get(id) {
                Some(fact) if fact.class_name == class_name => {}
                _ => return None,
            }
        }
    }
    Some(class_name.to_string())
}

/// The class of the `new` that a `Stmt::Let` in `stmts` binds to `want`.
/// `None` when the id is not bound by exactly one `Let { init: New }` here.
fn seeded_class_of_local(stmts: &[Stmt], want: u32) -> Option<&str> {
    let mut found: Option<&str> = None;
    let mut count = 0usize;
    walk_stmts(stmts, &mut |s| {
        if let Stmt::Let { id, init, .. } = s {
            if *id == want {
                count += 1;
                if let Some(Expr::New { class_name, .. }) = init.as_ref() {
                    found = Some(class_name.as_str());
                }
            }
        }
    });
    if count == 1 {
        found
    } else {
        None
    }
}

/// Collect the value expressions of every `return` **of this function** —
/// deliberately NOT descending into nested closure bodies, whose returns
/// belong to the closure. Returns `false` if a bare `return;` was found.
fn collect_own_returns<'a>(stmts: &'a [Stmt], out: &mut Vec<&'a Expr>) -> bool {
    let mut ok = true;
    walk_stmts(stmts, &mut |s| {
        if let Stmt::Return(r) = s {
            match r {
                Some(e) => out.push(e),
                None => ok = false,
            }
        }
    });
    ok
}

/// Statement walker over one function body, skipping nested closure bodies
/// (they are separate regions with their own returns and their own proof).
fn walk_stmts<'a>(stmts: &'a [Stmt], f: &mut impl FnMut(&'a Stmt)) {
    for s in stmts {
        f(s);
        match s {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                walk_stmts(then_branch, f);
                if let Some(eb) = else_branch {
                    walk_stmts(eb, f);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => walk_stmts(body, f),
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    walk_stmts(std::slice::from_ref(init.as_ref()), f);
                }
                walk_stmts(body, f);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                walk_stmts(body, f);
                if let Some(c) = catch {
                    walk_stmts(&c.body, f);
                }
                if let Some(fin) = finally {
                    walk_stmts(fin, f);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    walk_stmts(&c.body, f);
                }
            }
            Stmt::Labeled { body, .. } => walk_stmts(std::slice::from_ref(body.as_ref()), f),
            _ => {}
        }
    }
}

/// Caller-side seeding: add every `const r = <return-shape producer>(...)` in
/// this region to `candidates`, and return the set of ids so seeded.
///
/// Mirrors `find_new_candidates`' shape exactly — same exclusions (boxed,
/// module-global), same nesting, and no descent into closure bodies (each is
/// its own region).
pub(crate) fn find_return_shape_candidates(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    module_dispatch: &ModuleDispatchFacts,
    candidates: &mut HashMap<u32, String>,
) -> HashSet<u32> {
    let mut seeded = HashSet::new();
    walk_stmts(stmts, &mut |s| {
        let Stmt::Let {
            id,
            init: Some(Expr::Call { callee, .. }),
            ..
        } = s
        else {
            return;
        };
        if boxed_vars.contains(id) || module_globals.contains_key(id) {
            return;
        }
        // Only a direct `Expr::FuncRef` callee names one statically-known
        // function — the same resolution `clamp3_functions` / hot-callee
        // inlining already rely on. Anything computed could be rebound.
        let Expr::FuncRef(func_id) = callee.as_ref() else {
            return;
        };
        if let Some(class_name) = module_dispatch.return_shape_class(*func_id) {
            candidates.insert(*id, class_name.to_string());
            seeded.insert(*id);
        }
    });
    seeded
}

#[cfg(test)]
#[path = "ptr_shape_returns_tests.rs"]
mod tests;
