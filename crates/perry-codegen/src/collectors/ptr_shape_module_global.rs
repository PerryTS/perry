//! Representation-selection Phase 3b, rule 6 (#10769, #10803): admitting a
//! module-level `const` to the `Ptr<Shape>` proof.
//!
//! ## What this is for
//!
//! `collectors/ptr_shape.rs` proves a *function-local* receiver. Its seed
//! (`collectors/escape_check.rs:27`) filters every binding that
//! `codegen/module_globals_emit.rs` promoted to an `@perry_global_*` cell, and
//! a module-level binding is promoted the moment **any** function, method or
//! closure in the file so much as mentions it. The consequence measured on
//! v0.5.1631 is a cliff nobody wrote on purpose:
//!
//! ```text
//! const O = {a:1,b:2};
//! let h = 0; for (let k = 0; k < N; k++) h += O.a + O.b;   // 42.00 instr/iter
//! ```
//!
//! ```text
//! const O = {a:1,b:2};
//! function unused() { return O.a; }                         // never called
//! let h = 0; for (let k = 0; k < N; k++) h += O.a + O.b;   // 74.00 instr/iter
//! ```
//!
//! Identical object, identical loop, 1.76x apart because an unrelated function
//! names the binding. #10769 attributed this to storage — "a module binding has
//! a collector-rewritten global cell rather than a local slot", i.e.
//! representation work. It is not: the `Ptr<Shape>` access sites
//! (`expr/property_get/helpers.rs`) call `lower_expr` on the receiver and then
//! mask/gep/load, and never read a slot; and a module-global scalar read costs
//! 9.00 instructions against a function-local's 9.00. The storage is free. What
//! is missing is a proof that covers more than one region.
//!
//! ## Rule 6, and why each clause is load-bearing
//!
//! A module-level binding may seed `Ptr<Shape>` when ALL of the following hold.
//! Rules 1-5 of `ptr_shape.rs` still apply unchanged on top of these; this
//! module only decides which module-level bindings are allowed to *reach* them.
//!
//! **6a — provenance.** Exactly one `Stmt::Let { mutable: false }` binds the
//! id, it is a direct and unconditional element of `hir.init`, and its init is
//! an `Expr::New` (which is also what a closed object literal lowers to). The
//! `mutable: false` test is what excludes `var`, and with it every script-level
//! `var` that `hir.annexb_global_undefined_names` mirrors onto the global
//! object.
//!
//! **6b — no second name.** Exclusive reachability is the whole of the
//! optimisation, so every channel that could publish the object under a second
//! name disqualifies. For a module-level binding those channels are export
//! surfaces, and they are enumerable:
//!
//! * `hir.exports` — an exported binding gets EXTERNAL linkage plus a
//!   `perry_fn_<prefix>__<name>` getter (`module_globals_emit.rs`), so any
//!   importing module holds the object and can reshape it. Whole-program
//!   analysis would be needed and is not attempted.
//! * the module's own **namespace object** (`import * as ns`). Its entries are
//!   built from the export list (`run_pipeline.rs`), and a `LocalVar` entry is
//!   published as a live accessor whose getter loads `@perry_global_*` at read
//!   time — so `ns.O` IS the same object under a second name. A NON-exported
//!   binding never becomes a namespace entry, which is why 6b is stated in
//!   terms of exports and needs no separate namespace clause.
//! * `hir.script_global_functions` / `hir.annexb_global_undefined_names` —
//!   `globalThis` reflection. A module-level `const` is never reflected (only
//!   top-level `var` and bare function declarations in a Script are), but the
//!   sets are checked rather than reasoned about.
//!
//! `eval` needs no clause: Perry never executes a runtime code string
//! (`perry-hir/src/eval_classifier.rs`).
//!
//! **6c — containment, module-wide.** The existing `UseWalk` is run over EVERY
//! region of the module — `hir.init`, every function body, every constructor,
//! method, static method, getter, setter, computed member and field
//! initializer; nested closures are covered because the walk descends into
//! them. Rules 2-5 are applied unchanged. A module-level binding is visible to
//! all of those regions, so anything less than all of them is not a containment
//! proof.
//!
//! **6d — init-dominance, and this one has no analogue in the local case.**
//! For a function-local, provenance alone gives dominance: the `Stmt::Let`
//! precedes every use in the same body. For a module-level binding the `Let`
//! lives in `hir.init` and the uses live in other regions, so it dominates
//! nothing. Perry does not enforce TDZ on a module-level `const`:
//!
//! ```text
//! function f(){ return O.a; }
//! f();                      // node: ReferenceError (TDZ)
//! const O = {a:1,b:2};      // perry: TypeError from the GUARDED read path
//! ```
//!
//! The cell holds `TAG_UNDEFINED` and today's guarded diamond catches it. A
//! guard-free `Ptr<Shape>` read would instead compute
//! `TAG_UNDEFINED & POINTER_MASK`, gep and load — a wild load, on a program
//! that is merely buggy rather than malicious. Neither #10769 nor #10803 names
//! this obligation; both argued only about aliasing.
//!
//! Rule 6d discharges it structurally rather than with a runtime check: under
//! 6b the module exports nothing, so no other module can call into it and no
//! import cycle can observe it mid-init; and this clause requires that no
//! statement of `hir.init` before the `Let` can invoke user code — no
//! `Expr::Closure` and no `Expr::FuncRef` appears anywhere in the prefix (so
//! nothing can hold a user function to call, as a callee or as a callback),
//! and no `Expr::New` of a class declared in this module appears (so no user
//! constructor, field initializer or getter can run). Host and intrinsic calls
//! are permitted: they cannot reach a reader, because a reader is a user
//! function of THIS module and the prefix contains no reference to any.
//! Together: no user code at all runs before the `Let`, so no read of the
//! binding can execute before it is initialized, in any region.
//!
//! This is deliberately the conservative first increment. The population it
//! leaves out — a module that exports, or that calls a user function before the
//! declaration — needs a per-region entry check on the cell (one pointer-tag
//! test in the entry block, hoisted out of every loop, side-exiting to today's
//! guarded path) and is tracked separately. It is representation work, and
//! #10769's own closing line asks for it not to be attempted here.
//!
//! ## Why a region this pass forgets cannot become unsound
//!
//! The ids this pass admits are handed to every region through
//! `ModuleDispatchFacts`, and each region's own `collect_type_facts` re-runs
//! the containment walk over its own statements before consuming the fact. So
//! a region missing from [`module_regions`] below is a region that still
//! proves itself; the module-wide pass can only be MORE restrictive than the
//! union of the per-region ones, never less.

use std::collections::{HashMap, HashSet};

use perry_hir::{Class, Expr, Module, Stmt};

use super::ModuleDispatchFacts;
use super::PtrShapeLocal;

#[cfg(test)]
#[path = "ptr_shape_module_global_tests.rs"]
mod tests;

/// Every lowered region of a module, in a stable order.
///
/// Mirrors `collectors/spec_abi_sites.rs::scan_whole_module`, which is the
/// established enumeration for a module-wide proof. Nested closures are NOT
/// listed: `UseWalk` descends into `Expr::Closure` bodies itself.
pub(crate) fn module_regions(hir: &Module) -> Vec<&[Stmt]> {
    let mut regions: Vec<&[Stmt]> = Vec::new();
    for f in &hir.functions {
        regions.push(&f.body);
    }
    for c in &hir.classes {
        if let Some(ctor) = &c.constructor {
            regions.push(&ctor.body);
        }
        for m in c.methods.iter().chain(c.static_methods.iter()) {
            regions.push(&m.body);
        }
        for (_, g) in &c.getters {
            regions.push(&g.body);
        }
        for (_, s) in &c.setters {
            regions.push(&s.body);
        }
        for cm in &c.computed_members {
            regions.push(&cm.function.body);
        }
    }
    regions
}

/// Rule 6b: the module publishes nothing, so nothing outside it can name a
/// module-level binding or call into it before its own init has finished.
fn module_publishes_nothing(hir: &Module) -> bool {
    // `hir.script_global_functions` is deliberately NOT in this list. It is
    // the bare top-level `function` declarations a Script reflects onto the
    // global object, and reflecting `run` publishes `run`, not the record `run
    // happens to read — the binding itself is never reflected, because
    // GlobalDeclarationInstantiation gives a top-level `const` a declarative
    // binding, not a global property. Including it here denied every Script
    // with a top-level function, which is the entire population rule 6 exists
    // for.
    hir.exports.is_empty()
        && hir.exported_objects.is_empty()
        && hir.exported_functions.is_empty()
        && hir.exported_native_instances.is_empty()
        && hir.exported_func_return_native_instances.is_empty()
        && !hir.functions.iter().any(|f| f.is_exported)
}

/// Rule 6d: can allocating this class run a line of user code?
///
/// A closed object literal lowers to `Expr::New { class_name: "__AnonShape_…" }`
/// and is a pure record — no constructor, no field initializer, no accessor —
/// so allocating one cannot call anything. Testing the class rather than its
/// synthetic name keeps the answer right if that ever stops holding, and keeps
/// `const A = {…}; const B = {…};` from denying `B` on account of `A`.
fn class_can_execute_user_code(class: &Class) -> bool {
    class.constructor.is_some()
        || !class.getters.is_empty()
        || !class.setters.is_empty()
        || !class.computed_members.is_empty()
        || class.extends.is_some()
        || class.extends_expr.is_some()
        || class
            .fields
            .iter()
            .chain(class.static_fields.iter())
            .any(|f| f.init.is_some() || f.key_expr.is_some())
}

/// Rule 6d: can this expression transfer control to user code written in this
/// program? A `FuncRef` or `Closure` is enough on its own — holding a user
/// function value is what makes calling one possible, whether directly or as a
/// callback handed to a host builtin.
fn expr_can_reach_user_code(e: &Expr, class_names: &HashSet<&str>) -> bool {
    let mut found = false;
    let mut visit = |e: &Expr| {
        if found {
            return;
        }
        match e {
            Expr::FuncRef(_) | Expr::Closure { .. } | Expr::Await(_) => found = true,
            Expr::New { class_name, .. } if class_names.contains(class_name.as_str()) => {
                found = true
            }
            _ => {}
        }
    };
    visit(e);
    if !found {
        perry_hir::walker::walk_expr_children(e, &mut |c| {
            if !found {
                found = expr_can_reach_user_code(c, class_names);
            }
        });
    }
    found
}

fn stmt_can_reach_user_code(s: &Stmt, class_names: &HashSet<&str>) -> bool {
    let mut found = false;
    super::for_each_expr_in_stmts(std::slice::from_ref(s), &mut |e| {
        if !found && expr_can_reach_user_code(e, class_names) {
            found = true;
        }
    });
    found
}

/// The module-level bindings that rule 6 admits as `Ptr<Shape>` SEEDS, mapped
/// to the class their `Expr::New` provenance names.
///
/// Admission here is necessary, not sufficient: `ptr_shape.rs` still runs
/// rules 1-5 over every region before any fact is produced.
fn debug_enabled() -> bool {
    std::env::var("PERRY_MODULE_SHAPE_DEBUG").is_ok_and(|v| v != "0")
}

pub(crate) fn module_global_seed_candidates(hir: &Module) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    if !module_publishes_nothing(hir) {
        if debug_enabled() {
            eprintln!(
                "module-shape {}: publishes something (exports {} objects {} fns {} native {} \
                 funcnative {} exported_fn {})",
                hir.name,
                hir.exports.len(),
                hir.exported_objects.len(),
                hir.exported_functions.len(),
                hir.exported_native_instances.len(),
                hir.exported_func_return_native_instances.len(),
                hir.functions.iter().filter(|f| f.is_exported).count(),
            );
        }
        return out;
    }
    let class_names: HashSet<&str> = hir
        .classes
        .iter()
        .filter(|c| class_can_execute_user_code(c))
        .map(|c| c.name.as_str())
        .collect();
    // Rule 6a's "exactly one Let" is checked module-wide, not just over
    // `hir.init`: a same-id binding in any other region would be a second
    // provenance the walk would have to reconcile.
    let reassigned = super::reassigned_locals_in_module(hir);
    // A binding the lowering gave a box — above all a TDZ box, emitted for a
    // lexical binding "referenced (directly or via a closure) BEFORE their
    // declaration" (`perry-hir/src/ir/stmt.rs`) — is exactly the shape rule 6d
    // exists to keep out, and `PreallocateBoxes` also means the storage is a
    // heap cell rather than the binding's own. Either way it is not a seed.
    let mut boxed: HashSet<u32> = HashSet::new();
    for s in &hir.init {
        match s {
            Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => {
                boxed.extend(ids.iter().copied())
            }
            _ => {}
        }
    }
    let mut prefix_is_clean = true;
    for s in &hir.init {
        // Scan for eligibility BEFORE this statement's own contribution: a
        // `Let` is admitted on the prefix that precedes it.
        if let Stmt::Let {
            id,
            name,
            mutable: false,
            init: Some(Expr::New { class_name, .. }),
            ..
        } = s
        {
            let ok = prefix_is_clean
                && !reassigned.contains(id)
                && !boxed.contains(id)
                && !hir.annexb_global_undefined_names.iter().any(|n| n == name);
            if debug_enabled() {
                eprintln!(
                    "module-shape {}: `{name}` id {id} class {class_name} -> {} \
                     (prefix_clean {prefix_is_clean} reassigned {} boxed {} annexb {})",
                    hir.name,
                    if ok { "SEED" } else { "denied" },
                    reassigned.contains(id),
                    boxed.contains(id),
                    hir.annexb_global_undefined_names.iter().any(|n| n == name),
                );
            }
            if ok {
                out.insert(*id, class_name.clone());
            }
        }
        if prefix_is_clean && stmt_can_reach_user_code(s, &class_names) {
            prefix_is_clean = false;
            if debug_enabled() {
                eprintln!("module-shape {}: prefix dirtied by {s:?}", hir.name);
            }
        }
    }
    out
}

/// Rule 6: the module-level bindings whose `Ptr<Shape>` proof holds over every
/// region of the module.
///
/// Run once per module, before any region is compiled, and installed on
/// [`ModuleDispatchFacts`] so every region sees the same verdict.
pub(crate) fn collect_module_global_shape_locals(
    hir: &Module,
    classes: &HashMap<String, &Class>,
    module_dispatch: &ModuleDispatchFacts,
) -> HashMap<u32, PtrShapeLocal> {
    let mut seeds = module_global_seed_candidates(hir);
    if seeds.is_empty() {
        return HashMap::new();
    }
    let regions = module_regions(hir);
    // Rule 6 applies to a binding that OUTLIVES module init, which in Perry is
    // exactly a binding some other region names: that reference is what
    // promotes it to an `@perry_global_*` cell
    // (`codegen/module_globals_emit.rs`) and what the seed filter then drops.
    // A binding only module init names is not promoted, and its own region pass
    // already proves it WITH the numeric-field claim rule 6 has to stand down
    // from -- seeding it here would trade 42 instructions for 47.
    let mut referenced_outside_init: HashSet<u32> = HashSet::new();
    for region in &regions {
        super::for_each_expr_in_stmts(region, &mut |e| {
            if let Expr::LocalGet(id) = e {
                if seeds.contains_key(id) {
                    referenced_outside_init.insert(*id);
                }
            }
        });
    }
    seeds.retain(|id, _| referenced_outside_init.contains(id));
    if debug_enabled() {
        eprintln!(
            "module-shape {}: {} seed(s) {:?}",
            hir.name,
            seeds.len(),
            seeds
        );
    }
    if seeds.is_empty() {
        return HashMap::new();
    }
    let mut facts = super::ptr_shape::collect_module_wide_shape_locals(
        hir,
        classes,
        module_dispatch,
        &seeds,
        &regions,
    );
    // Only the module-level seeds are this pass's business; `hir.init`'s own
    // ordinary locals are proven by their region's pass, with the numeric
    // proof this one deliberately stands down from.
    facts.retain(|id, _| seeds.contains_key(id));
    if debug_enabled() {
        eprintln!(
            "module-shape {}: {} proven of {} seed(s)",
            hir.name,
            facts.len(),
            seeds.len()
        );
    }
    facts
}
