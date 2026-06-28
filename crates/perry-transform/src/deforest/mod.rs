//! Interprocedural deforestation: fuse "callee allocates array, fills,
//! returns; caller iterates and copies into outer array" into a single
//! pass where the callee writes directly into an accumulator passed by
//! the caller.
//!
//! ## The pattern
//!
//! Producer (callee):
//! ```ignore
//! function f(p1, p2, ...) {
//!     const out = [];
//!     // ... pushes into `out` ...
//!     // ... possibly recursive call `f(...)` whose result is consumed
//!     //     into `out` ...
//!     return out;
//! }
//! ```
//!
//! Consumer (caller):
//! ```ignore
//! const child = f(args);
//! for (let j = 0; j < child.length; j++) outer.push(child[j]);
//! // child has no other use after the loop
//! ```
//!
//! ## The transformation
//!
//! 1. Add a trailing `__deforest_out: Array<T>` parameter to f.
//! 2. Replace `const out = []` with no-op (the parameter IS the
//!    accumulator).
//! 3. Replace every `out.push(...)` / `out.something` with the param.
//! 4. Replace `return out` with `return undefined`.
//! 5. **Recursive calls inside f**: pass the param through directly.
//! 6. **Consumer call sites**: rewrite the consume-loop pattern to
//!    `f(args, outer)` — no temporary array, no copy loop.
//! 7. **Non-consumer call sites** (e.g. top-level `const all = f(...)`):
//!    rewrite to `const all = []; f(args, all);` so callers that need
//!    the array as a value still get it.
//!
//! ## Why this matters
//!
//! On ABC451D-shaped recursive workloads, every recursive call
//! allocates a fresh array, fills it, returns it, the caller iterates
//! and copies elements into ITS array. Each level multiplies the
//! allocation pressure. After deforestation, ONE array is shared
//! across the entire recursion — the inner recursion writes directly
//! into the top-level accumulator. Manually-rewritten ABC451D drops
//! from ~3.2 s to ~0.74 s on Apple M-series (4.3× faster, within 1.8×
//! of Bun).
//!
//! ## Scope
//!
//! Intra-module only. Cross-module deforestation requires propagating
//! the rewritten signature through every importer and is filed as a
//! separate follow-up.
//!
//! Limitations (transformation bails when these are observed):
//! - Producer's `out` is referenced anywhere besides `push` / member
//!   reads of `length`, `map` produce-style or read-only methods, or
//!   the final `return out`.
//! - Producer has multiple return paths some of which don't return
//!   `out`.
//! - Producer's body assigns `out` (`out = something_else`).
//! - Producer is async or a generator (state-machine flattening would
//!   complicate the rewrite; not blocked by spec, just out of MVP).
//! - A call site has the producer's result `await`-ed (Promise wrap).
//! - Consumer's `outer` is modified between `const child = f()` and
//!   the consume loop in a way that aliases `child`.

use perry_hir::{Expr, Function, Module, Stmt};
use perry_types::{FuncId, LocalId, Type};
use std::collections::{HashMap, HashSet};

mod call_sites;
mod detect;
mod out_usage;
mod producer_rewrite;
mod scan;
mod walk;

#[cfg(test)]
mod tests;

pub use call_sites::{rewrite_call_sites_in_stmts, rewrite_call_sites_in_stmts_with_local_pass};
pub use detect::{analyze_producer, body_has_closure, detect_producers, stmt_contains_return};
pub use out_usage::OutUsageAnalyzer;
pub use producer_rewrite::{rewrite_producer_body, SubstituteLocal};
pub use scan::{scan_funcref_misuses, scan_producers_used_in_closures, scan_unsafe_call_sites};
pub use walk::{
    max_local_id, max_local_id_for_func, stmt_references_local, walk_expr_children,
    walk_expr_children_mut,
};

/// Per-producer information collected during detection.
#[derive(Debug, Clone)]
pub struct ProducerInfo {
    /// LocalId of the `let out = []` binding inside the producer body.
    pub out_local_id: LocalId,
    /// Number of original parameters (before we add the out-param).
    /// Recursive call rewrites need to know this to position the new
    /// arg correctly.
    pub original_param_count: usize,
    /// Element type of the accumulator. Inferred from the producer's
    /// return type if known, else `Any`. Used for the new param's
    /// declared type.
    pub elem_ty: Type,
}

/// Public entry point. Mutates `module` in place: rewrites every
/// detected producer function to take an accumulator parameter, and
/// rewrites every detected call site to pass an accumulator and elide
/// the consume loop. Functions that don't match the producer shape
/// are left unchanged; modules with no matching functions are no-ops.
pub fn run(module: &mut Module) {
    let producers = detect_producers(module);
    if producers.is_empty() {
        return;
    }

    if std::env::var("PERRY_DEFOREST_DEBUG").is_ok() {
        for (id, p) in &producers {
            eprintln!(
                "[deforest] producer fn_id={} name={} out_local={} param_count={}",
                id,
                module
                    .functions
                    .iter()
                    .find(|f| f.id == *id)
                    .map(|f| f.name.as_str())
                    .unwrap_or("?"),
                p.out_local_id,
                p.original_param_count
            );
        }
    }

    // Allocate fresh LocalIds for the synthetic out-parameter on each
    // producer. The id space is module-wide, so we walk the module to
    // find max + 1 once and bump from there.
    let mut next_local = max_local_id(module) + 1;
    let mut out_param_ids: HashMap<FuncId, LocalId> = HashMap::new();
    let mut producer_ids: Vec<FuncId> = producers.keys().copied().collect();
    producer_ids.sort_unstable();
    for id in producer_ids {
        out_param_ids.insert(id, next_local);
        next_local += 1;
    }

    // Phase 2: rewrite producer bodies — add the param, swap `out`
    // references for the param, drop the return.
    for func in &mut module.functions {
        if let Some(info) = producers.get(&func.id) {
            let out_param = out_param_ids[&func.id];
            rewrite_producer_body(func, info, out_param, &producers, &out_param_ids);
        }
    }

    // Phase 3: rewrite call sites in module-init and every function
    // body. The producer's own body already had its recursive call
    // sites rewritten by phase 2 — phase 3 covers callers that are
    // NOT the producer itself (top-level scripts, sibling helpers,
    // etc.).
    rewrite_call_sites_in_stmts(
        &mut module.init,
        &producers,
        &out_param_ids,
        &mut next_local,
    );
    for func in &mut module.functions {
        // Skip the producers themselves — their bodies were already
        // rewritten in phase 2 (which knows the param substitution).
        if producers.contains_key(&func.id) {
            continue;
        }
        rewrite_call_sites_in_stmts(&mut func.body, &producers, &out_param_ids, &mut next_local);
    }

    // Class member bodies (constructors, methods, accessors, static methods)
    // are equally valid producer call sites — `detect_producers` already
    // scans them for unsafe usages, so a producer admitted here may have its
    // only `let x = f(...)` call inside a method. Without rewriting those
    // sites the call keeps its original arity while the producer's signature
    // gained the `__deforest_out` param; codegen then passes `undefined` for
    // the missing arg and the body operates on a non-array, SIGSEGVing (same
    // class of arity-mismatch miscompile as the in-closure bail-out, #5136 —
    // but here the call sites are ordinary statement bodies we can rewrite
    // rather than bail on). Producers are only ever free functions
    // (`analyze_producer` runs on `module.functions`), so no skip is needed.
    for class in &mut module.classes {
        if let Some(ctor) = &mut class.constructor {
            rewrite_call_sites_in_stmts(
                &mut ctor.body,
                &producers,
                &out_param_ids,
                &mut next_local,
            );
        }
        for method in &mut class.methods {
            rewrite_call_sites_in_stmts(
                &mut method.body,
                &producers,
                &out_param_ids,
                &mut next_local,
            );
        }
        for (_, getter) in &mut class.getters {
            rewrite_call_sites_in_stmts(
                &mut getter.body,
                &producers,
                &out_param_ids,
                &mut next_local,
            );
        }
        for (_, setter) in &mut class.setters {
            rewrite_call_sites_in_stmts(
                &mut setter.body,
                &producers,
                &out_param_ids,
                &mut next_local,
            );
        }
        for method in &mut class.static_methods {
            rewrite_call_sites_in_stmts(
                &mut method.body,
                &producers,
                &out_param_ids,
                &mut next_local,
            );
        }
    }
}
