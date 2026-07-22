//! Function and Method Inlining Pass for Perry HIR
//!
//! Split from a single 5,818-line file into topical sub-modules in
//! v0.5.1019 to satisfy the file-size CI gate. mod.rs is the entry
//! point: it owns the public API (`inline_functions`, `MethodCandidate`,
//! the cross-module `gather_*` helpers re-exported below) and the
//! top-level driver loop; sub-modules hold the analysis, dispatch,
//! and rewrite phases.

mod analysis;
mod call_inliner;
mod clamp;
mod closure_analysis;
mod cross_module;
mod exact_receivers;
mod factory_specialize;
mod imul;
mod substitute;
mod super_detect;

// Public re-exports (explicit named — globs don't propagate transitively
// through `pub(crate) use crate::inline::*` consumers).
pub use cross_module::{
    gather_cross_module_anon_classes, gather_cross_module_methods,
    gather_cross_module_methods_with_extern_imports, is_cross_module_safe,
};

// Internal-to-crate re-exports for cross-sibling access via `use super::*;`.
pub(crate) use analysis::{
    find_max_local_id_in_module, is_inlinable, is_inlinable_method, method_lookup_is_unshadowed,
};
pub(crate) use call_inliner::inline_calls_in_stmts;
pub(crate) use clamp::{is_clamp3, is_clamp_u8};
pub(crate) use closure_analysis::{
    body_contains_closure_capturing, body_contains_super_call, body_references_dynamic_this,
    collect_closure_captured_local_ids, collect_declared_local_ids, collect_mutated_local_ids,
    find_max_local_id, has_simple_control_flow, is_pure_function,
    method_body_blocks_this_substitution,
};
pub(crate) use exact_receivers::{
    apply_exact_receiver_stmt_effect, apply_exact_receiver_stmt_effects,
    intersect_exact_receiver_facts, invalidate_exact_receivers_for_expr,
    kill_referenced_exact_receivers,
};
pub(crate) use factory_specialize::specialize_captured_class_factories;
pub(crate) use imul::{detect_math_imul_polyfill, rewrite_imul_calls_in_stmts};
pub(crate) use substitute::{
    collect_body_local_ids, substitute_locals, substitute_locals_in_stmts, substitute_this,
    substitute_this_in_stmts,
};
#[cfg(test)]
pub(crate) use super_detect::MAX_INLINE_EXPR_RECURSION_DEPTH;
pub(crate) use super_detect::{enter_inline_expr_recursion, method_contains_lexical_super};

use perry_hir::types::{FuncId, LocalId, Type};
use perry_hir::{Class, Expr, Function, Module, Stmt};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Maximum number of statements for a function to be considered for inlining
pub(crate) const MAX_INLINE_STMTS: usize = 10;

/// Information about a method that can be inlined
#[derive(Clone, Debug)]
pub struct MethodCandidate {
    pub func: Function,
    /// The index of the `this` parameter (if present)
    pub this_param_id: Option<LocalId>,
    /// True only when a direct prototype-method inline preserves normal
    /// property lookup for this method name. Instance fields, computed fields,
    /// and accessors can all shadow `obj.method` before the prototype method.
    pub method_lookup_safe: bool,
    /// `Expr::ExternFuncRef` names referenced inside the body, paired with
    /// the `resolved_path` of the source module that originally exported
    /// each name. Empty for methods harvested via `gather_cross_module_methods`
    /// (those reject any extern-ref). Non-empty for methods harvested via
    /// `gather_cross_module_methods_with_extern_imports`, where the inliner
    /// uses the `resolved_path` to add any missing import to the destination
    /// module's `hir.imports` so the codegen's `import_function_prefixes`
    /// table can dispatch the cross-module call (`perry_fn_<source_prefix>__<name>`).
    pub required_extern_imports: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExactReceiverFact {
    pub(crate) class_name: String,
}

pub(crate) type ExactReceiverFacts = HashMap<LocalId, ExactReceiverFact>;
/// Inline small functions and methods in the module.
///
/// `extra_methods` carries inlinable methods harvested from previously-
/// compiled modules. The driver in `collect_modules.rs` assembles it from
/// `ctx.native_modules` so that when a method body in module M calls
/// `imported_world.set(...)`, the inliner can look up `("World", "set")`
/// even though `World` isn't defined in M. Only "cross-module safe"
/// methods (no FuncRef / ExternFuncRef / GlobalGet — i.e. nothing whose
/// resolution would dangle in another module's symbol space) appear in
/// `extra_methods`; the safety filter is `gather_cross_module_methods`
/// below.
pub fn inline_functions(
    module: &mut Module,
    extra_methods: &HashMap<(String, String), MethodCandidate>,
    extra_class_fields: &HashMap<(String, String), String>,
    extra_anon_classes: &HashMap<String, &Class>,
) {
    // ── Cross-module anon-class propagation ──
    // Anon-shape classes (`__AnonShape_<hash>`) are content-addressed by
    // their canonical shape key, so the same shape across modules produces
    // the same name. But each source module materializes its own class
    // definition and the cross-module method inliner copies bodies that
    // reference these classes by name. If the destination module never
    // synthesized that shape itself, codegen later finds no class entry
    // for `__AnonShape_<hash>` and falls into a generic-object path —
    // which silently drops fields (the symptom that masked
    // `world.query([T]).length === 0` after `world.sync()`).
    //
    // Pull in every anon class referenced by any cross-module-inlinable
    // candidate that the destination module hasn't already synthesized
    // locally. Hash-named, so dedup is by-name and definitionally safe.
    {
        use perry_hir::walker::walk_expr_children;
        fn collect_anon_refs(stmts: &[Stmt], out: &mut HashSet<String>) {
            for s in stmts {
                walk_stmt_exprs(s, &mut |e| collect_anon_refs_in_expr(e, out));
            }
        }
        fn collect_anon_refs_in_expr(e: &Expr, out: &mut HashSet<String>) {
            if let Expr::New { class_name, .. } = e {
                if class_name.starts_with("__AnonShape_") {
                    out.insert(class_name.clone());
                }
            }
            walk_expr_children(e, &mut |c| collect_anon_refs_in_expr(c, out));
        }
        fn walk_stmt_exprs(s: &Stmt, f: &mut impl FnMut(&Expr)) {
            match s {
                Stmt::Let { init, .. } => {
                    if let Some(e) = init {
                        f(e);
                    }
                }
                Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => f(e),
                Stmt::Return(None) | Stmt::Break | Stmt::Continue => {}
                Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    f(condition);
                    for s in then_branch {
                        walk_stmt_exprs(s, f);
                    }
                    if let Some(eb) = else_branch {
                        for s in eb {
                            walk_stmt_exprs(s, f);
                        }
                    }
                }
                Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                    f(condition);
                    for s in body {
                        walk_stmt_exprs(s, f);
                    }
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    if let Some(s) = init {
                        walk_stmt_exprs(s, f);
                    }
                    if let Some(c) = condition {
                        f(c);
                    }
                    if let Some(u) = update {
                        f(u);
                    }
                    for s in body {
                        walk_stmt_exprs(s, f);
                    }
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    f(discriminant);
                    for c in cases {
                        if let Some(t) = &c.test {
                            f(t);
                        }
                        for s in &c.body {
                            walk_stmt_exprs(s, f);
                        }
                    }
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    for s in body {
                        walk_stmt_exprs(s, f);
                    }
                    if let Some(c) = catch {
                        for s in &c.body {
                            walk_stmt_exprs(s, f);
                        }
                    }
                    if let Some(fi) = finally {
                        for s in fi {
                            walk_stmt_exprs(s, f);
                        }
                    }
                }
                Stmt::Labeled { body, .. } => walk_stmt_exprs(body.as_ref(), f),
                Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
            }
        }

        let mut needed: HashSet<String> = HashSet::new();
        for cand in extra_methods.values() {
            collect_anon_refs(&cand.func.body, &mut needed);
        }
        let already_present: HashSet<String> =
            module.classes.iter().map(|c| c.name.clone()).collect();
        // Anon-shape ctor params + body Lets are minted by the SOURCE
        // module's `fresh_local`, so the cloned class carries those
        // source-module ids verbatim into the destination. Those ids
        // can collide with destination ids that participate in the
        // destination's `module_boxed_vars` (closures over a mutated
        // local elsewhere in the destination), and the codegen for the
        // ctor body's `LocalGet(param.id)` then routes through
        // `js_box_get` on a slot that holds a plain value — not a
        // box pointer — silently producing NaN for that field. Symptom:
        // `[PERRY WARN] js_box_get: invalid box pointer` once per call
        // site (limited by the warn counter), with the affected
        // anon-shape literal field reading back as NaN at runtime.
        // Bench3 (perf-comprehensive) printed `Sum X = 0` because every
        // archetype's `componentTypes=[null]` post-corruption.
        // Remap each imported anon-class's ctor params + body Let ids
        // to fresh destination ids above the destination's max — that
        // way they can't intersect with whatever boxed_vars are
        // computed later for this module.
        let mut next_fresh_id = find_max_local_id_in_module(module) + 1;
        let mut needed_names: Vec<String> = needed.into_iter().collect();
        needed_names.sort();
        for name in needed_names {
            if already_present.contains(&name) {
                continue;
            }
            if let Some(src_cls) = extra_anon_classes.get(&name) {
                let mut cloned = (**src_cls).clone();
                if let Some(ctor) = &mut cloned.constructor {
                    let mut remap: HashMap<LocalId, Expr> = HashMap::new();
                    for p in ctor.params.iter_mut() {
                        let new_id = next_fresh_id;
                        next_fresh_id += 1;
                        remap.insert(p.id, Expr::LocalGet(new_id));
                        p.id = new_id;
                    }
                    let body_local_ids = collect_body_local_ids(&ctor.body);
                    for old_id in body_local_ids {
                        remap.entry(old_id).or_insert_with(|| {
                            let new_id = next_fresh_id;
                            next_fresh_id += 1;
                            Expr::LocalGet(new_id)
                        });
                    }
                    substitute_locals_in_stmts(&mut ctor.body, &remap, &mut next_fresh_id);
                }
                module.classes.push(cloned);
            }
        }
    }

    // Issue #740: captured-class-factory specialization.
    //
    // Pattern:
    //   function makeFactory<T>(tag: T) {
    //     class Inner { readonly _tag = tag; }   // captures outer `tag`
    //     return Inner;
    //   }
    //   const Cls = makeFactory("MyTag");
    //   const inst = new Cls();
    //   inst._tag  // expected: "MyTag"
    //
    // `class Inner` is hoisted to `module.classes` during HIR lowering with a
    // synthesized `__perry_cap_<outer_id>` ctor param + matching field, and
    // field initializers that reference outer locals are rewritten to read
    // those ctor params. The `tag` value reaches the field only when the
    // ctor receives it as an argument — which the standalone `new Cls()`
    // call site can't supply because `Cls` is just `ClassRef("Inner")` and
    // the outer scope's `tag` doesn't exist at the module level.
    //
    // Fix: specialize the class per call site. For each
    //   `Let { name: X, init: Call(FuncRef(f), args) }`
    // where `f`'s body is `Return(Some(ClassRef(C)))` and `C` has
    // `__perry_cap_*` ctor params, clone `C` to `C_inline_<n>` with the
    // captures baked in as constants (substituting the ctor-param ids in
    // method/getter/setter/field-init bodies with the call's matching arg).
    // Then drop the capture ctor param + matching field + the assignment in
    // the ctor body. Replace the Let's init with `ClassRef(C_inline_<n>)`.
    // The standalone `new Cls()` then works with no args because Cls is now
    // bound to a class with no captures left to bind.
    //
    // Runs BEFORE the main inliner so the regular pass sees the rewritten
    // (non-factory-Call) inits. Note that the factory function itself may
    // still be eligible for normal inlining — that's fine: with the rewrite
    // above the Let's init is no longer a Call, so the regular path is a
    // no-op for the rewritten sites.
    specialize_captured_class_factories(module);

    // Phases 0 + 1 fused (Tier 4.1, v0.5.335): single iteration over
    // module.functions collects both Math.imul polyfill ids AND
    // inlinable-function candidates. Pre-Tier-4 these were two separate
    // `module.functions.iter()` passes back-to-back. Math.imul detection
    // and `is_inlinable` are independent reads with no ordering
    // dependency, so fusing is safe and saves one full module scan.
    let mut imul_polyfill_ids: HashSet<FuncId> = HashSet::new();
    let mut func_candidates: HashMap<FuncId, Function> = HashMap::new();
    for f in module.functions.iter() {
        if detect_math_imul_polyfill(f) {
            imul_polyfill_ids.insert(f.id);
        }
        // (Issue #436 plan #1) Clamp-pattern functions (`if (v<lo)
        // return lo; if (v>hi) return hi; return v;` and the 1-arg
        // `clampU8` variant) are deliberately NOT inlined here. The
        // codegen recognizes them by id and emits a branchless
        // `@llvm.smin.i32` / `@llvm.smax.i32` chain at every call site
        // (`crates/perry-codegen/src/expr.rs::lower_expr_as_i32` for
        // i32-required contexts, plus the f64-context arm in
        // `lower_call.rs::lower_call`). Inlining replaces the
        // `Expr::Call { callee: FuncRef(clamp_fn_id) }` shape with a
        // `do { ... } while (false)` block carrying `if/break` rewrites
        // of the original `return`s — IR shape that LLVM's auto-vec
        // refuses to lift. Skipping the inline keeps the recognizable
        // pattern intact for codegen.
        if is_clamp3(f) || is_clamp_u8(f) {
            continue;
        }
        if is_inlinable(f) {
            func_candidates.insert(f.id, f.clone());
        }
    }

    // Phase 0 mutation pass: rewrite imul call sites in every body.
    // Must run BEFORE the inliner expands those calls, so the polyfill
    // body is never decomposed into 5+ operations — the codegen emits a
    // single `mul i32` instead. Conditional on at least one polyfill
    // being detected so we don't traverse for nothing.
    if !imul_polyfill_ids.is_empty() {
        rewrite_imul_calls_in_stmts(&mut module.init, &imul_polyfill_ids);
        for func in &mut module.functions {
            if !imul_polyfill_ids.contains(&func.id) {
                rewrite_imul_calls_in_stmts(&mut func.body, &imul_polyfill_ids);
            }
        }
        for class in &mut module.classes {
            if let Some(ref mut ctor) = class.constructor {
                rewrite_imul_calls_in_stmts(&mut ctor.body, &imul_polyfill_ids);
            }
            for method in &mut class.methods {
                rewrite_imul_calls_in_stmts(&mut method.body, &imul_polyfill_ids);
            }
        }
    }

    // Phases 2 + 3 fused (Tier 4.1): single iteration over
    // module.classes builds both the inlinable-method map AND the
    // class-name lookup. class_names is unconditional (covers every
    // class regardless of native_extends), so it lives at the top of
    // the loop body before the native_extends short-circuit for method
    // collection.
    let mut method_candidates: HashMap<(String, String), MethodCandidate> = HashMap::new();
    let mut class_names: HashMap<String, String> = HashMap::new();
    // (class_name, field_name) → field's class type (when the field is
    // declared as `Type::Named(class)`). Populated from this module's local
    // classes plus any cross-module classes the driver supplies via
    // `extra_class_fields`. Used by the receiver-class resolver to decide
    // whether to inline `someLocal.field.method(...)` chains.
    let mut class_field_types: HashMap<(String, String), String> = HashMap::new();
    for (k, v) in extra_class_fields {
        class_field_types.insert(k.clone(), v.clone());
    }
    for class in &module.classes {
        for f in &class.fields {
            if let Type::Named(field_class) = &f.ty {
                class_field_types.insert((class.name.clone(), f.name.clone()), field_class.clone());
            }
        }
    }
    // Build the `(name, resolved_path) -> Import` map once for deduping.
    // For each Named import in dest, we know which (name, path) is already
    // satisfied. Anything required by an admitted candidate that isn't here
    // gets appended below.
    let mut dest_named_imports: HashSet<(String, String)> = HashSet::new();
    let mut dest_resolved_paths: HashSet<String> = HashSet::new();
    for imp in &module.imports {
        if let Some(p) = &imp.resolved_path {
            dest_resolved_paths.insert(p.clone());
            for spec in &imp.specifiers {
                if let perry_hir::ImportSpecifier::Named { local, .. } = spec {
                    dest_named_imports.insert((local.clone(), p.clone()));
                }
            }
        }
    }
    // Source-of-truth: for each (name, source_path) combination requested by
    // an admitted candidate, look up the matching `Import` from extra_methods
    // (we need the original `Import` shape — `is_native`, `module_kind` —
    // so the codegen processes the new entry the same way it processes a
    // user-written import). Since we only have the resolved_path (not the
    // original source string or module_kind) on the candidate side, we
    // reconstruct a minimal Import here. `is_native = false` because the
    // strict-cross-module-safe check already excluded NativeMethodCall and
    // other native-only patterns; `module_kind = NativeCompiled` because
    // that's the only category the codegen consults for
    // `import_function_prefixes`.
    let mut needed_imports: BTreeMap<String, Vec<String>> = BTreeMap::new();
    method_candidates.extend(extra_methods.iter().filter_map(|(k, v)| {
        // If any required (name, path) is missing from dest, queue an import.
        // We always admit when the path is reachable from the destination —
        // if dest has no import that resolves to that path, we synthesize
        // one. (A path that names a module not in `ctx.native_modules` would
        // still fail at codegen, but that's a pre-existing issue; the
        // harvester wouldn't populate `required_extern_imports` from such a
        // path.)
        for (name, path) in &v.required_extern_imports {
            if !dest_named_imports.contains(&(name.clone(), path.clone())) {
                needed_imports
                    .entry(path.clone())
                    .or_default()
                    .push(name.clone());
            }
        }
        Some((k.clone(), v.clone()))
    }));
    // Synthesize import entries for the needed names. Group per source-path.
    for (path, mut names) in needed_imports {
        names.sort();
        names.dedup();
        // If dest already has an Import for this resolved_path, append the
        // names there to keep the imports list clean. Otherwise create a
        // fresh Import.
        let existing_idx = module.imports.iter().position(|imp| {
            imp.resolved_path
                .as_deref()
                .is_some_and(|p| p == path.as_str())
        });
        match existing_idx {
            Some(idx) => {
                for name in names {
                    if !module.imports[idx]
                        .specifiers
                        .iter()
                        .any(|s| matches!(s, perry_hir::ImportSpecifier::Named { local, .. } if local == &name))
                    {
                        module.imports[idx]
                            .specifiers
                            .push(perry_hir::ImportSpecifier::Named {
                                imported: name.clone(),
                                local: name,
                            });
                    }
                }
            }
            None => {
                module.imports.push(perry_hir::Import {
                    source: path.clone(),
                    specifiers: names
                        .into_iter()
                        .map(|name| perry_hir::ImportSpecifier::Named {
                            imported: name.clone(),
                            local: name,
                        })
                        .collect(),
                    is_native: false,
                    module_kind: perry_hir::ModuleKind::NativeCompiled,
                    resolved_path: Some(path),
                    type_only: false,
                    is_dynamic: false,
                    is_dynamic_target: false,
                    is_deferred_require: false,
                    is_adopted_require: false,
                });
            }
        }
    }
    let _ = dest_resolved_paths; // kept for future deduping diagnostics
    for class in &module.classes {
        class_names.insert(class.name.clone(), class.name.clone());

        // Don't inline methods from classes with native parents (e.g.,
        // EventEmitter) — the `this` reference needs special handling
        // in those contexts. The class_name lookup above still records
        // the type so other passes can reference it.
        if class.native_extends.is_some() {
            continue;
        }
        for method in &class.methods {
            if is_inlinable_method(method) {
                // Methods don't have 'this' as a parameter in the HIR;
                // they access it via Expr::This. So this_param_id is
                // None.
                method_candidates.insert(
                    (class.name.clone(), method.name.clone()),
                    MethodCandidate {
                        func: method.clone(),
                        this_param_id: None,
                        method_lookup_safe: method_lookup_is_unshadowed(
                            &module.classes,
                            &class.name,
                            &method.name,
                        ),
                        required_extern_imports: Vec::new(),
                    },
                );
            }
        }
    }

    // Compute a MODULE-WIDE max LocalId used as the starting point for all
    // inliner-allocated local IDs. CRITICAL: LocalIds are globally unique across
    // the whole module (HIR lowering uses a single `fresh_local` counter), so any
    // newly allocated ID must exceed the max used ANYWHERE in the module — not
    // just in the current scope (init / function body / method body). Otherwise
    // the inliner can produce a module-level Let whose id collides with a class
    // method's parameter id, and the subsequent module_var_data_ids loader in
    // codegen silently skips loading the global (because `locals.contains_key(id)`
    // is already true for the method parameter), leaving the method reading the
    // wrong value from the class field.
    let module_max_id = find_max_local_id_in_module(module);

    // Phase 4: Inline calls in init statements.
    // Method calls are always safe (they access `this.field` via pointer indirection).
    // Standalone functions are safe ONLY if they are "pure" — i.e. they don't read or
    // write module-level variables. Module-level variables are cached in locals during
    // compile_init, so an inlined function that reads a module variable modified by a
    // prior call would see the stale cached value. Pure functions (which only use their
    // own parameters and body locals) avoid this problem entirely.
    {
        let pure_func_candidates: HashMap<FuncId, Function> = func_candidates
            .iter()
            .filter(|(_, f)| is_pure_function(f))
            .map(|(id, f)| (*id, f.clone()))
            .collect();
        let mut next_local_id = module_max_id + 1;
        let mut local_types: HashMap<LocalId, String> = HashMap::new();
        let mut exact_receiver_facts = ExactReceiverFacts::new();
        inline_calls_in_stmts(
            &mut module.init,
            &pure_func_candidates,
            &method_candidates,
            &class_names,
            &mut local_types,
            &mut exact_receiver_facts,
            &mut next_local_id,
            None,
            &class_field_types,
        );
    }

    // Phase 5: Inline calls in function bodies
    //
    // Each function body now uses a private ID counter that starts after the
    // module-wide max AND any IDs previously allocated by the init-phase inliner.
    // We maintain a running `next_module_id` so each phase advances the shared
    // counter, preventing collisions between phases.
    let mut next_module_id = module_max_id + 1;
    // Advance past any IDs consumed by the init phase by re-scanning the module.
    next_module_id = next_module_id.max(find_max_local_id_in_module(module) + 1);
    for func in &mut module.functions {
        if func_candidates.contains_key(&func.id) {
            continue;
        }
        let mut local_id = next_module_id;
        let mut local_types: HashMap<LocalId, String> = HashMap::new();
        let mut exact_receiver_facts = ExactReceiverFacts::new();
        // Add function parameters to local_types
        for param in &func.params {
            if let Type::Named(class_name) = &param.ty {
                local_types.insert(param.id, class_name.clone());
            }
        }
        inline_calls_in_stmts(
            &mut func.body,
            &func_candidates,
            &method_candidates,
            &class_names,
            &mut local_types,
            &mut exact_receiver_facts,
            &mut local_id,
            None,
            &class_field_types,
        );
        next_module_id = local_id;
    }

    // Phase 6: Inline calls in class method bodies. Pass the enclosing class
    // name so `this.someMethod()` calls inside a method body (which the HIR
    // represents as `Expr::Call { callee: PropertyGet { object: Expr::This,
    // property } }`) can be resolved against `method_candidates` and inlined.
    // This is the load-bearing case for the ECS perf workloads, where
    // `World.set` calls `this.resolveSetOperation(...)` 10k times per round
    // — without inlining each call goes through `js_native_call_method`
    // dispatch + heap-allocates the returned `{entityId, componentType,
    // component}` literal.
    for class in &mut module.classes {
        let class_name = class.name.clone();
        for method in &mut class.methods {
            // Skip if this method is itself a candidate (avoid recursion)
            if method_candidates.contains_key(&(class_name.clone(), method.name.clone())) {
                continue;
            }
            let mut local_id = next_module_id;
            let mut local_types: HashMap<LocalId, String> = HashMap::new();
            let mut exact_receiver_facts = ExactReceiverFacts::new();
            for param in &method.params {
                if let Type::Named(class_name) = &param.ty {
                    local_types.insert(param.id, class_name.clone());
                }
            }
            inline_calls_in_stmts(
                &mut method.body,
                &func_candidates,
                &method_candidates,
                &class_names,
                &mut local_types,
                &mut exact_receiver_facts,
                &mut local_id,
                Some(&class_name),
                &class_field_types,
            );
            next_module_id = local_id;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::{ImportSpecifier, ModuleKind};

    fn function(id: FuncId, body: Vec<Stmt>) -> Function {
        Function {
            id,
            name: format!("f{}", id),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Void,
            body,
            is_async: false,
            is_generator: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
            is_strict: false,
        }
    }

    fn candidate(
        id: FuncId,
        body: Vec<Stmt>,
        required_extern_imports: Vec<(String, String)>,
    ) -> MethodCandidate {
        MethodCandidate {
            func: function(id, body),
            this_param_id: None,
            method_lookup_safe: true,
            required_extern_imports,
        }
    }

    fn anon_class(id: u32, name: &str) -> Class {
        Class {
            id,
            name: name.to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: None,
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: Vec::new(),
            constructor: None,
            methods: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            computed_members: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
            is_nested: false,
        }
    }

    fn anon_new(name: &str) -> Stmt {
        Stmt::Expr(Expr::New {
            class_name: name.to_string(),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
            cap_args_appended: 0,
        })
    }

    #[test]
    fn cross_module_synthetic_imports_are_sorted() {
        let mut module = Module::new("dest");
        let mut extra_methods = HashMap::new();
        extra_methods.insert(
            ("B".to_string(), "m".to_string()),
            candidate(
                1,
                Vec::new(),
                vec![
                    ("z".to_string(), "/z.ts".to_string()),
                    ("a".to_string(), "/a.ts".to_string()),
                ],
            ),
        );
        extra_methods.insert(
            ("A".to_string(), "m".to_string()),
            candidate(
                2,
                Vec::new(),
                vec![
                    ("b".to_string(), "/b.ts".to_string()),
                    ("a2".to_string(), "/a.ts".to_string()),
                ],
            ),
        );

        inline_functions(
            &mut module,
            &extra_methods,
            &HashMap::new(),
            &HashMap::new(),
        );

        let sources: Vec<&str> = module.imports.iter().map(|i| i.source.as_str()).collect();
        assert_eq!(sources, vec!["/a.ts", "/b.ts", "/z.ts"]);
        assert!(module.imports.iter().all(|i| {
            !i.is_native
                && i.module_kind == ModuleKind::NativeCompiled
                && i.resolved_path.as_deref() == Some(i.source.as_str())
                && !i.type_only
                && !i.is_dynamic
                && !i.is_dynamic_target
        }));

        let first_specifiers: Vec<(&str, &str)> = module.imports[0]
            .specifiers
            .iter()
            .map(|s| match s {
                ImportSpecifier::Named { imported, local } => (imported.as_str(), local.as_str()),
                _ => panic!("expected named import"),
            })
            .collect();
        assert_eq!(first_specifiers, vec![("a", "a"), ("a2", "a2")]);
    }

    #[test]
    fn cross_module_anon_classes_are_appended_in_name_order() {
        let mut module = Module::new("dest");
        let mut extra_methods = HashMap::new();
        extra_methods.insert(
            ("B".to_string(), "m".to_string()),
            candidate(1, vec![anon_new("__AnonShape_bbb")], Vec::new()),
        );
        extra_methods.insert(
            ("A".to_string(), "m".to_string()),
            candidate(2, vec![anon_new("__AnonShape_aaa")], Vec::new()),
        );
        let anon_bbb = anon_class(2, "__AnonShape_bbb");
        let anon_aaa = anon_class(1, "__AnonShape_aaa");
        let mut extra_anon_classes = HashMap::new();
        extra_anon_classes.insert("__AnonShape_bbb".to_string(), &anon_bbb);
        extra_anon_classes.insert("__AnonShape_aaa".to_string(), &anon_aaa);

        inline_functions(
            &mut module,
            &extra_methods,
            &HashMap::new(),
            &extra_anon_classes,
        );

        let class_names: Vec<&str> = module.classes.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(class_names, vec!["__AnonShape_aaa", "__AnonShape_bbb"]);
    }
}
