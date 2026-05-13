//! Function and Method Inlining Pass for Perry HIR
//!
//! This module inlines small functions and methods at their call sites to eliminate
//! call overhead and enable further optimizations.

use perry_hir::walker::{walk_expr_children, walk_expr_children_mut};
use perry_hir::{BinaryOp, Class, Expr, Function, Module, Stmt};
use perry_types::{FuncId, LocalId, Type};
use std::collections::{HashMap, HashSet};

/// Maximum number of statements for a function to be considered for inlining
const MAX_INLINE_STMTS: usize = 10;

/// Information about a method that can be inlined
#[derive(Clone, Debug)]
pub struct MethodCandidate {
    pub func: Function,
    /// The index of the `this` parameter (if present)
    pub this_param_id: Option<LocalId>,
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
    extra_anon_classes: &HashMap<String, Class>,
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
                Stmt::PreallocateBoxes(_) => {}
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
        for name in &needed {
            if already_present.contains(name) {
                continue;
            }
            if let Some(src_cls) = extra_anon_classes.get(name) {
                let mut cloned = src_cls.clone();
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
    let mut needed_imports: HashMap<String, Vec<String>> = HashMap::new();
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
            if is_inlinable(method) {
                // Methods don't have 'this' as a parameter in the HIR;
                // they access it via Expr::This. So this_param_id is
                // None.
                method_candidates.insert(
                    (class.name.clone(), method.name.clone()),
                    MethodCandidate {
                        func: method.clone(),
                        this_param_id: None,
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
        inline_calls_in_stmts(
            &mut module.init,
            &pure_func_candidates,
            &method_candidates,
            &class_names,
            &mut local_types,
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
                &mut local_id,
                Some(&class_name),
                &class_field_types,
            );
            next_module_id = local_id;
        }
    }
}

/// Find the maximum LocalId used ANYWHERE in the module: init statements,
/// function bodies, class constructors, class method bodies, class field
/// initializers, and closure bodies nested inside any of the above. Used to
/// compute a safe starting point for inliner-allocated local IDs so they don't
/// collide with existing HIR ids anywhere in the module.
fn find_max_local_id_in_module(module: &Module) -> LocalId {
    let mut max_id: LocalId = 0;
    max_id = max_id.max(find_max_local_id(&module.init));
    for func in &module.functions {
        for param in &func.params {
            max_id = max_id.max(param.id);
        }
        max_id = max_id.max(find_max_local_id(&func.body));
    }
    for class in &module.classes {
        if let Some(ref ctor) = class.constructor {
            for param in &ctor.params {
                max_id = max_id.max(param.id);
            }
            max_id = max_id.max(find_max_local_id(&ctor.body));
        }
        for method in &class.methods {
            for param in &method.params {
                max_id = max_id.max(param.id);
            }
            max_id = max_id.max(find_max_local_id(&method.body));
        }
        for (_, getter) in &class.getters {
            for param in &getter.params {
                max_id = max_id.max(param.id);
            }
            max_id = max_id.max(find_max_local_id(&getter.body));
        }
        for (_, setter) in &class.setters {
            for param in &setter.params {
                max_id = max_id.max(param.id);
            }
            max_id = max_id.max(find_max_local_id(&setter.body));
        }
        for method in &class.static_methods {
            for param in &method.params {
                max_id = max_id.max(param.id);
            }
            max_id = max_id.max(find_max_local_id(&method.body));
        }
    }
    max_id
}

/// Check if a function is suitable for inlining
fn is_inlinable(func: &Function) -> bool {
    // Don't inline async functions
    if func.is_async {
        return false;
    }

    // Don't inline generator functions. The body uses `yield` and (often) a
    // terminating `return value` to drive the state machine that
    // `transform_generators` later builds. Inlining the body into the caller
    // erases that contract: the `yield` exprs leak into the caller's
    // statement list and the call expression collapses to the function's
    // last `return` value (or `undefined`), so `gen()` no longer produces
    // an iterator — it produces the bare return value. Issue #457.
    if func.is_generator {
        return false;
    }

    // Don't inline functions with captures (closures)
    if !func.captures.is_empty() {
        return false;
    }

    // Don't inline functions with rest parameters. The current call-site
    // arg-handling maps each formal param to one actual arg via param_map;
    // a rest param needs the trailing args bundled into a synthetic
    // `Expr::Array(...)` setup_stmt, which the inliner does not emit.
    // Without that, only the first trailing arg ends up bound to the
    // rest param (as a scalar), and the body's `parts.length` /
    // `parts[i]` / `parts.join(...)` then operate on whatever scalar
    // value happened to be passed — strings get treated as
    // single-element arrays, numbers as raw doubles, etc.
    if func.params.iter().any(|p| p.is_rest) {
        return false;
    }

    // Don't inline functions that are too large
    if func.body.len() > MAX_INLINE_STMTS {
        return false;
    }

    // Check for simple patterns
    if !has_simple_control_flow(&func.body) {
        return false;
    }

    // Don't inline functions that return closures capturing parameters
    // When inlined, the parameter IDs won't exist in the outer context
    let param_ids: std::collections::HashSet<LocalId> = func.params.iter().map(|p| p.id).collect();
    if body_contains_closure_capturing(&func.body, &param_ids) {
        return false;
    }

    // Don't inline methods containing super.method() or super() calls.
    // These rely on the enclosing class context (ThisContext with parent_class)
    // which is lost once the body is inlined into the caller.
    if body_contains_super_call(&func.body) {
        return false;
    }

    // Don't inline functions that call themselves. The single-Return-of-Call
    // pattern in `try_inline_simple_call` (and the multi-Let-then-Return
    // pattern) substitutes the body verbatim; when the body is
    // `return self(args)` the substituted result IS another call to `self`,
    // which `inline_calls_in_expr` immediately re-inlines, and the recursion
    // unbounded → perry-main stack overflow (issue #733: 2-line repro
    // `function f(): any { return f(); }; f();` blew the 64 MB compiler
    // stack on Windows). The infinite-recursion case has no useful
    // inlining anyway (we can't unroll the call into a finite expression
    // tree). Reject any function whose body contains a call whose callee
    // is its own FuncRef.
    if body_calls_func(&func.body, func.id) {
        return false;
    }

    true
}

/// Check if `stmts` contains any `Expr::Call { callee: FuncRef(target_id) }`,
/// recursively. Stops at closure boundaries — a self-reference inside a nested
/// closure is a value-position read, not a same-frame recursive tail, and the
/// closure body becomes a separate function at codegen.
fn body_calls_func(stmts: &[Stmt], target_id: FuncId) -> bool {
    fn check_expr(expr: &Expr, target_id: FuncId) -> bool {
        match expr {
            Expr::Call { callee, args, .. } => {
                if let Expr::FuncRef(fid) = callee.as_ref() {
                    if *fid == target_id {
                        return true;
                    }
                }
                check_expr(callee, target_id) || args.iter().any(|a| check_expr(a, target_id))
            }
            Expr::Binary { left, right, .. }
            | Expr::Logical { left, right, .. }
            | Expr::Compare { left, right, .. } => {
                check_expr(left, target_id) || check_expr(right, target_id)
            }
            Expr::Unary { operand, .. } => check_expr(operand, target_id),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                check_expr(condition, target_id)
                    || check_expr(then_expr, target_id)
                    || check_expr(else_expr, target_id)
            }
            Expr::Array(elements) => elements.iter().any(|e| check_expr(e, target_id)),
            Expr::IndexGet { object, index } => {
                check_expr(object, target_id) || check_expr(index, target_id)
            }
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                check_expr(object, target_id)
                    || check_expr(index, target_id)
                    || check_expr(value, target_id)
            }
            Expr::PropertyGet { object, .. } => check_expr(object, target_id),
            Expr::PropertySet { object, value, .. } => {
                check_expr(object, target_id) || check_expr(value, target_id)
            }
            Expr::LocalSet(_, value) => check_expr(value, target_id),
            // Closure bodies are NOT walked: their calls happen in a different
            // frame at runtime. A nested closure that calls the outer is fine
            // for inlining purposes (the closure body itself won't be inlined
            // here — it's a separate codegen unit).
            _ => false,
        }
    }
    fn check_stmt(stmt: &Stmt, target_id: FuncId) -> bool {
        match stmt {
            Stmt::Let {
                init: Some(expr), ..
            } => check_expr(expr, target_id),
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                check_expr(expr, target_id)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition, target_id)
                    || then_branch.iter().any(|s| check_stmt(s, target_id))
                    || else_branch
                        .as_ref()
                        .is_some_and(|b| b.iter().any(|s| check_stmt(s, target_id)))
            }
            Stmt::While { condition, body } => {
                check_expr(condition, target_id) || body.iter().any(|s| check_stmt(s, target_id))
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_some_and(|i| check_stmt(i, target_id))
                    || condition.as_ref().is_some_and(|c| check_expr(c, target_id))
                    || update.as_ref().is_some_and(|u| check_expr(u, target_id))
                    || body.iter().any(|s| check_stmt(s, target_id))
            }
            _ => false,
        }
    }
    stmts.iter().any(|s| check_stmt(s, target_id))
}

/// Returns true iff `body` only references symbols whose resolution stays
/// stable across modules. Concretely: the body must not contain any of
/// `Expr::FuncRef` (callee bound to a same-module function id), `Expr::ExternFuncRef`
/// (an import — fine in the source module but not necessarily present in the
/// destination), `Expr::GlobalGet`/`Expr::GlobalSet` (module-level state),
/// or `Expr::NativeModuleRef`. Method calls (`Expr::Call` whose callee is a
/// `PropertyGet` on `This` / a local / a property), `Expr::NativeMethodCall`,
/// property/index access, control flow, primitives, object/array literals,
/// and arithmetic are all OK — runtime dispatch via the class registry works
/// uniformly across modules. Used by `gather_cross_module_methods` to filter
/// the candidates the driver feeds back in via `extra_methods`.
pub fn is_cross_module_safe(body: &[Stmt]) -> bool {
    fn check_expr(expr: &Expr) -> bool {
        match expr {
            // The disqualifying variants — anything tied to a particular
            // module's symbol table.
            Expr::FuncRef(_)
            | Expr::ExternFuncRef { .. }
            | Expr::GlobalGet(_)
            | Expr::GlobalSet(_, _)
            | Expr::NativeModuleRef(_) => false,
            // Closures are out of scope for cross-module inlining: the
            // closure body has its own LocalIds, captures lists, and may
            // reference symbols we can't safely move.
            Expr::Closure { .. } => false,
            // Everything else: descend into all sub-expressions via the
            // central walker.
            other => {
                let mut ok = true;
                walk_expr_children(other, &mut |child| {
                    if !check_expr(child) {
                        ok = false;
                    }
                });
                ok
            }
        }
    }
    fn check_stmt(s: &Stmt) -> bool {
        match s {
            Stmt::Let { init, .. } => init.as_ref().is_none_or(check_expr),
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => check_expr(e),
            Stmt::Return(None) | Stmt::Break | Stmt::Continue => true,
            Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition)
                    && then_branch.iter().all(check_stmt)
                    && else_branch
                        .as_ref()
                        .is_none_or(|eb| eb.iter().all(check_stmt))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                check_expr(condition) && body.iter().all(check_stmt)
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_none_or(|s| check_stmt(s))
                    && condition.as_ref().is_none_or(check_expr)
                    && update.as_ref().is_none_or(check_expr)
                    && body.iter().all(check_stmt)
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                check_expr(discriminant)
                    && cases.iter().all(|c| {
                        c.test.as_ref().is_none_or(check_expr) && c.body.iter().all(check_stmt)
                    })
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                body.iter().all(check_stmt)
                    && catch.as_ref().is_none_or(|c| c.body.iter().all(check_stmt))
                    && finally.as_ref().is_none_or(|f| f.iter().all(check_stmt))
            }
            Stmt::Labeled { body, .. } => check_stmt(body.as_ref()),
            Stmt::PreallocateBoxes(_) => true,
        }
    }
    body.iter().all(check_stmt)
}

/// Harvest inlinable, cross-module-safe methods from `module`. Used by the
/// compile driver to assemble the `extra_methods` map that subsequent modules
/// receive in `inline_functions`. Only methods that pass both `is_inlinable`
/// (the existing per-module gate) and `is_cross_module_safe` (the symbol-
/// frontier gate) make it into the result. Constructors, getters, setters,
/// and static methods are excluded — those have either non-trivial dispatch
/// semantics or a class-tied receiver that cross-module callers can't supply.
/// Harvest content-addressed anon-shape classes (`__AnonShape_<hash>`)
/// from a module. The driver merges these across all prior modules and
/// passes the result to `inline_functions` as `extra_anon_classes` so the
/// destination module gets any class definitions referenced by inlined
/// cross-module method bodies. Hash naming makes dedup-by-name correct
/// (same shape from any module → same name → identical class definition).
pub fn gather_cross_module_anon_classes(module: &Module) -> HashMap<String, Class> {
    let mut out: HashMap<String, Class> = HashMap::new();
    for class in &module.classes {
        if class.name.starts_with("__AnonShape_") {
            out.insert(class.name.clone(), class.clone());
        }
    }
    out
}

pub fn gather_cross_module_methods(module: &Module) -> HashMap<(String, String), MethodCandidate> {
    let mut out: HashMap<(String, String), MethodCandidate> = HashMap::new();
    let nonexported = collect_nonexported_class_names(module);
    for class in &module.classes {
        if class.native_extends.is_some() {
            continue;
        }
        for method in &class.methods {
            if !is_inlinable(method) {
                continue;
            }
            if !is_cross_module_safe(&method.body) {
                continue;
            }
            if body_references_class_in_set(&method.body, &nonexported) {
                continue;
            }
            out.insert(
                (class.name.clone(), method.name.clone()),
                MethodCandidate {
                    func: method.clone(),
                    this_param_id: None,
                    required_extern_imports: Vec::new(),
                },
            );
        }
    }
    out
}

/// Like `gather_cross_module_methods`, but additionally permits methods that
/// invoke `Expr::ExternFuncRef` — recording each referenced name in
/// `required_extern_imports` so the inline-time safety check can verify the
/// destination module imports the same names before inlining.
///
/// `Expr::FuncRef` (same-module function-id reference) and `Expr::GlobalGet`
/// remain disallowed: function-id and module-globals can't survive a cross-
/// module move at all (the source module's symbol space isn't visible).
/// Closures and `Expr::NativeModuleRef` also remain disallowed.
///
/// The hot motivator here is `World.resolveSetOperation` — its body invokes
/// the imported `getDetailedIdType` (an ExternFuncRef in the World module),
/// which the strict filter rejected. With this looser filter the method
/// becomes a candidate; the inline-time check then permits it iff the
/// destination module also imports `getDetailedIdType`.
pub fn gather_cross_module_methods_with_extern_imports(
    module: &Module,
) -> HashMap<(String, String), MethodCandidate> {
    let mut out: HashMap<(String, String), MethodCandidate> = HashMap::new();
    let nonexported = collect_nonexported_class_names(module);
    // Pre-build a name → resolved_path map from this module's imports so we
    // can resolve each ExternFuncRef in a method body to its source-of-truth.
    // The destination module needs that resolved_path to add the matching
    // Import (the codegen's import_function_prefixes lookup keys on it).
    let mut import_name_to_path: HashMap<String, String> = HashMap::new();
    for imp in &module.imports {
        let Some(path) = imp.resolved_path.clone() else {
            continue;
        };
        for spec in &imp.specifiers {
            if let perry_hir::ImportSpecifier::Named { local, .. } = spec {
                import_name_to_path.insert(local.clone(), path.clone());
            }
        }
    }
    for class in &module.classes {
        if class.native_extends.is_some() {
            continue;
        }
        for method in &class.methods {
            if !is_inlinable(method) {
                continue;
            }
            let mut extern_names: Vec<String> = Vec::new();
            if !is_cross_module_safe_with_externs(&method.body, &mut extern_names) {
                continue;
            }
            // Refs #486: a method body that constructs a non-exported local
            // class (`new InnerPrivate()`) can't be safely inlined into another
            // module — the destination module won't have `InnerPrivate` in its
            // class registry, so `lower_new("InnerPrivate")` falls into the
            // placeholder path that allocates an empty object with class_id=0.
            // Subsequent `inst.method()` dispatch then can't find a vtable
            // entry and falls through to NULL_OBJECT_BYTES. Keep the call as
            // a real cross-module method call (`bl perry_method_<src>__C__m`)
            // so the source module's codegen — which DOES have the class
            // metadata — emits the correct inline-alloc with the right
            // class_id.
            if body_references_class_in_set(&method.body, &nonexported) {
                continue;
            }
            extern_names.sort();
            extern_names.dedup();
            // Resolve each extern name against this module's imports. If
            // any name is unresolvable (it's referenced via ExternFuncRef
            // but doesn't appear as a Named import in this module — could
            // happen for built-ins like `setTimeout` that get
            // ExternFuncRef'd without a corresponding import statement),
            // skip the candidate entirely. The inline-time path needs a
            // concrete source path to copy over.
            let mut required: Vec<(String, String)> = Vec::with_capacity(extern_names.len());
            let mut resolvable = true;
            for name in &extern_names {
                if let Some(p) = import_name_to_path.get(name) {
                    required.push((name.clone(), p.clone()));
                } else {
                    resolvable = false;
                    break;
                }
            }
            if !resolvable {
                continue;
            }
            out.insert(
                (class.name.clone(), method.name.clone()),
                MethodCandidate {
                    func: method.clone(),
                    this_param_id: None,
                    required_extern_imports: required,
                },
            );
        }
    }
    out
}

/// Variant of `is_cross_module_safe` that allows `Expr::ExternFuncRef` and
/// records each referenced name into `extern_names`. Used by
/// `gather_cross_module_methods_with_extern_imports`. Same disqualifying
/// rules for FuncRef / GlobalGet / NativeModuleRef / Closure.
fn is_cross_module_safe_with_externs(body: &[Stmt], extern_names: &mut Vec<String>) -> bool {
    fn check_expr(expr: &Expr, extern_names: &mut Vec<String>) -> bool {
        match expr {
            Expr::FuncRef(_)
            | Expr::GlobalGet(_)
            | Expr::GlobalSet(_, _)
            | Expr::NativeModuleRef(_) => false,
            Expr::Closure { .. } => false,
            Expr::ExternFuncRef { name, .. } => {
                extern_names.push(name.clone());
                true
            }
            other => {
                let mut ok = true;
                walk_expr_children(other, &mut |child| {
                    if !check_expr(child, extern_names) {
                        ok = false;
                    }
                });
                ok
            }
        }
    }
    fn check_stmt(s: &Stmt, extern_names: &mut Vec<String>) -> bool {
        match s {
            Stmt::Let { init, .. } => init.as_ref().is_none_or(|e| check_expr(e, extern_names)),
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => check_expr(e, extern_names),
            Stmt::Return(None) | Stmt::Break | Stmt::Continue => true,
            Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition, extern_names)
                    && then_branch.iter().all(|s| check_stmt(s, extern_names))
                    && else_branch
                        .as_ref()
                        .is_none_or(|eb| eb.iter().all(|s| check_stmt(s, extern_names)))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                check_expr(condition, extern_names)
                    && body.iter().all(|s| check_stmt(s, extern_names))
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_none_or(|s| check_stmt(s, extern_names))
                    && condition
                        .as_ref()
                        .is_none_or(|e| check_expr(e, extern_names))
                    && update.as_ref().is_none_or(|e| check_expr(e, extern_names))
                    && body.iter().all(|s| check_stmt(s, extern_names))
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                check_expr(discriminant, extern_names)
                    && cases.iter().all(|c| {
                        c.test.as_ref().is_none_or(|e| check_expr(e, extern_names))
                            && c.body.iter().all(|s| check_stmt(s, extern_names))
                    })
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                body.iter().all(|s| check_stmt(s, extern_names))
                    && catch
                        .as_ref()
                        .is_none_or(|c| c.body.iter().all(|s| check_stmt(s, extern_names)))
                    && finally
                        .as_ref()
                        .is_none_or(|f| f.iter().all(|s| check_stmt(s, extern_names)))
            }
            Stmt::Labeled { body, .. } => check_stmt(body.as_ref(), extern_names),
            Stmt::PreallocateBoxes(_) => true,
        }
    }
    body.iter().all(|s| check_stmt(s, extern_names))
}

/// Collect the names of every class declared in `module` that is NOT exported.
/// These are the classes that can't safely cross a module boundary via the
/// inline-method-body copy path: callers in other modules don't see them in
/// their `imported_classes` table, so any `Expr::New { class_name }` /
/// `Expr::ClassRef` / `Expr::StaticFieldGet` / etc. that names one of these
/// classes will lose its class metadata at codegen time. Refs #486.
///
/// The `__AnonShape_*` content-addressed shapes are deliberately INCLUDED in
/// the set despite never being marked `is_exported` — but the inliner already
/// propagates them via `extra_anon_classes` so the destination module
/// synthesizes the same definition. We exclude them here so methods that
/// `new __AnonShape_<hash>()` keep their inlinability.
fn collect_nonexported_class_names(module: &Module) -> HashSet<String> {
    let mut set = HashSet::new();
    for c in &module.classes {
        if c.is_exported {
            // Refs #486: even for an EXPORTED class, the inner self-binding
            // alias from `var X = class _X` (recorded in `c.aliases`) is NOT
            // exported under the inner name — only the outer name `X` is
            // visible cross-module. A method body that constructs `new _X()`
            // (e.g. hono `Node.insert` doing `new _Node()` inside an exported
            // `class Node = class _Node`) can't be inlined into a destination
            // module, because the destination only sees `Node` in its
            // `imported_classes` table — `_Node` falls into the
            // `js_object_alloc(0, 0)` placeholder path. Add the alias names
            // to the rejection set so methods that reference them stay as
            // real cross-module method calls.
            for alias in &c.aliases {
                set.insert(alias.clone());
            }
            continue;
        }
        if c.name.starts_with("__AnonShape_") {
            continue;
        }
        set.insert(c.name.clone());
        for alias in &c.aliases {
            set.insert(alias.clone());
        }
    }
    set
}

/// Returns true iff `stmts` references any class whose name is in `set`.
/// Walks every Expr variant that carries a `class_name` string. Used by
/// the cross-module method gathering passes to reject candidates whose
/// body would dangle (or worse: silently fall to a class_id=0 placeholder)
/// after being copied into a destination module.
fn body_references_class_in_set(stmts: &[Stmt], set: &HashSet<String>) -> bool {
    fn check_expr(expr: &Expr, set: &HashSet<String>) -> bool {
        match expr {
            Expr::New { class_name, .. }
            | Expr::ClassRef(class_name)
            | Expr::StaticFieldGet { class_name, .. }
            | Expr::StaticFieldSet { class_name, .. }
            | Expr::ClassStaticSymbolSet { class_name, .. }
            | Expr::RegisterClassParentDynamic { class_name, .. }
            | Expr::StaticMethodCall { class_name, .. } => {
                if set.contains(class_name) {
                    return true;
                }
            }
            _ => {}
        }
        let mut hit = false;
        walk_expr_children(expr, &mut |child| {
            if check_expr(child, set) {
                hit = true;
            }
        });
        hit
    }
    fn check_stmt(s: &Stmt, set: &HashSet<String>) -> bool {
        match s {
            Stmt::Let { init, .. } => init.as_ref().is_some_and(|e| check_expr(e, set)),
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => check_expr(e, set),
            Stmt::Return(None) | Stmt::Break | Stmt::Continue => false,
            Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => false,
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition, set)
                    || then_branch.iter().any(|s| check_stmt(s, set))
                    || else_branch
                        .as_ref()
                        .is_some_and(|eb| eb.iter().any(|s| check_stmt(s, set)))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                check_expr(condition, set) || body.iter().any(|s| check_stmt(s, set))
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_some_and(|s| check_stmt(s, set))
                    || condition.as_ref().is_some_and(|e| check_expr(e, set))
                    || update.as_ref().is_some_and(|e| check_expr(e, set))
                    || body.iter().any(|s| check_stmt(s, set))
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                check_expr(discriminant, set)
                    || cases.iter().any(|c| {
                        c.test.as_ref().is_some_and(|e| check_expr(e, set))
                            || c.body.iter().any(|s| check_stmt(s, set))
                    })
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                body.iter().any(|s| check_stmt(s, set))
                    || catch
                        .as_ref()
                        .is_some_and(|c| c.body.iter().any(|s| check_stmt(s, set)))
                    || finally
                        .as_ref()
                        .is_some_and(|f| f.iter().any(|s| check_stmt(s, set)))
            }
            Stmt::Labeled { body, .. } => check_stmt(body.as_ref(), set),
            Stmt::PreallocateBoxes(_) => false,
        }
    }
    stmts.iter().any(|s| check_stmt(s, set))
}

/// Check if a body contains Expr::SuperCall or Expr::SuperMethodCall (recursively).
fn body_contains_super_call(stmts: &[Stmt]) -> bool {
    fn check_expr(expr: &Expr) -> bool {
        match expr {
            Expr::SuperCall(_) | Expr::SuperMethodCall { .. } => true,
            Expr::Binary { left, right, .. }
            | Expr::Logical { left, right, .. }
            | Expr::Compare { left, right, .. } => check_expr(left) || check_expr(right),
            Expr::Unary { operand, .. } => check_expr(operand),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => check_expr(condition) || check_expr(then_expr) || check_expr(else_expr),
            Expr::Call { callee, args, .. } => check_expr(callee) || args.iter().any(check_expr),
            Expr::Array(elements) => elements.iter().any(check_expr),
            Expr::IndexGet { object, index } => check_expr(object) || check_expr(index),
            Expr::IndexSet {
                object,
                index,
                value,
            } => check_expr(object) || check_expr(index) || check_expr(value),
            Expr::PropertyGet { object, .. } => check_expr(object),
            Expr::PropertySet { object, value, .. } => check_expr(object) || check_expr(value),
            Expr::LocalSet(_, value) => check_expr(value),
            _ => false,
        }
    }

    fn check_stmt(stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Let {
                init: Some(expr), ..
            } => check_expr(expr),
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => check_expr(expr),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition)
                    || then_branch.iter().any(check_stmt)
                    || else_branch
                        .as_ref()
                        .is_some_and(|b| b.iter().any(check_stmt))
            }
            Stmt::While { condition, body } => check_expr(condition) || body.iter().any(check_stmt),
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_some_and(|i| check_stmt(i))
                    || condition.as_ref().is_some_and(check_expr)
                    || update.as_ref().is_some_and(check_expr)
                    || body.iter().any(check_stmt)
            }
            _ => false,
        }
    }

    stmts.iter().any(check_stmt)
}

/// Check if statements contain a closure that captures any of the given local IDs
fn body_contains_closure_capturing(
    stmts: &[Stmt],
    captured_ids: &std::collections::HashSet<LocalId>,
) -> bool {
    fn check_expr(expr: &Expr, captured_ids: &std::collections::HashSet<LocalId>) -> bool {
        match expr {
            Expr::Closure { captures, body, .. } => {
                // Check if any capture is in the set of IDs we're looking for
                for capture_id in captures {
                    if captured_ids.contains(capture_id) {
                        return true;
                    }
                }
                // Also check the closure body for nested closures
                body_contains_closure_capturing(body, captured_ids)
            }
            Expr::Binary { left, right, .. }
            | Expr::Logical { left, right, .. }
            | Expr::Compare { left, right, .. } => {
                check_expr(left, captured_ids) || check_expr(right, captured_ids)
            }
            Expr::Unary { operand, .. } => check_expr(operand, captured_ids),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                check_expr(condition, captured_ids)
                    || check_expr(then_expr, captured_ids)
                    || check_expr(else_expr, captured_ids)
            }
            Expr::Call { callee, args, .. } => {
                check_expr(callee, captured_ids) || args.iter().any(|a| check_expr(a, captured_ids))
            }
            Expr::Array(elements) => elements.iter().any(|e| check_expr(e, captured_ids)),
            Expr::IndexGet { object, index } => {
                check_expr(object, captured_ids) || check_expr(index, captured_ids)
            }
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                check_expr(object, captured_ids)
                    || check_expr(index, captured_ids)
                    || check_expr(value, captured_ids)
            }
            Expr::PropertyGet { object, .. } => check_expr(object, captured_ids),
            Expr::PropertySet { object, value, .. } => {
                check_expr(object, captured_ids) || check_expr(value, captured_ids)
            }
            Expr::LocalSet(_, value) => check_expr(value, captured_ids),
            _ => false,
        }
    }

    fn check_stmt(stmt: &Stmt, captured_ids: &std::collections::HashSet<LocalId>) -> bool {
        match stmt {
            Stmt::Let {
                init: Some(expr), ..
            } => check_expr(expr, captured_ids),
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                check_expr(expr, captured_ids)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition, captured_ids)
                    || then_branch.iter().any(|s| check_stmt(s, captured_ids))
                    || else_branch
                        .as_ref()
                        .is_some_and(|b| b.iter().any(|s| check_stmt(s, captured_ids)))
            }
            Stmt::While { condition, body } => {
                check_expr(condition, captured_ids)
                    || body.iter().any(|s| check_stmt(s, captured_ids))
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_some_and(|i| check_stmt(i, captured_ids))
                    || condition
                        .as_ref()
                        .is_some_and(|c| check_expr(c, captured_ids))
                    || update.as_ref().is_some_and(|u| check_expr(u, captured_ids))
                    || body.iter().any(|s| check_stmt(s, captured_ids))
            }
            _ => false,
        }
    }

    stmts.iter().any(|s| check_stmt(s, captured_ids))
}

/// Check if a function is "pure" for init-inlining purposes: its body only
/// references its own parameters and locally-declared variables.  No GlobalGet,
/// GlobalSet, ExternFuncRef, or NativeMethodCall.  This makes it safe to inline
/// into module init context where module-level variables are cached in locals.
fn is_pure_function(func: &Function) -> bool {
    let mut known_ids: std::collections::HashSet<LocalId> = std::collections::HashSet::new();
    for p in &func.params {
        known_ids.insert(p.id);
    }
    // Collect all Let-declared IDs in the body
    let body_ids = collect_body_local_ids(&func.body);
    for id in body_ids {
        known_ids.insert(id);
    }

    fn expr_is_pure(e: &Expr, known: &std::collections::HashSet<LocalId>) -> bool {
        match e {
            Expr::GlobalGet(_) | Expr::GlobalSet(_, _) => false,
            Expr::ExternFuncRef { .. } => false,
            Expr::NativeMethodCall { .. } => false,
            Expr::LocalGet(id) | Expr::Update { id, .. } => known.contains(id),
            Expr::LocalSet(id, val) => known.contains(id) && expr_is_pure(val, known),
            Expr::Binary { left, right, .. }
            | Expr::Logical { left, right, .. }
            | Expr::Compare { left, right, .. } => {
                expr_is_pure(left, known) && expr_is_pure(right, known)
            }
            Expr::Unary { operand, .. } => expr_is_pure(operand, known),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                expr_is_pure(condition, known)
                    && expr_is_pure(then_expr, known)
                    && expr_is_pure(else_expr, known)
            }
            Expr::Call { callee, args, .. } => {
                expr_is_pure(callee, known) && args.iter().all(|a| expr_is_pure(a, known))
            }
            Expr::Array(elems) => elems.iter().all(|e| expr_is_pure(e, known)),
            Expr::IndexGet { object, index } => {
                expr_is_pure(object, known) && expr_is_pure(index, known)
            }
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                expr_is_pure(object, known)
                    && expr_is_pure(index, known)
                    && expr_is_pure(value, known)
            }
            Expr::PropertyGet { object, .. } => expr_is_pure(object, known),
            Expr::PropertySet { object, value, .. } => {
                expr_is_pure(object, known) && expr_is_pure(value, known)
            }
            // Leaf expressions with no variable references are always pure
            Expr::Integer(_)
            | Expr::Number(_)
            | Expr::Bool(_)
            | Expr::String(_)
            | Expr::Null
            | Expr::Undefined
            | Expr::FuncRef(_)
            | Expr::This => true,
            // For anything else we haven't explicitly handled, be conservative
            _ => true,
        }
    }

    fn stmt_is_pure(s: &Stmt, known: &std::collections::HashSet<LocalId>) -> bool {
        match s {
            Stmt::Let { init: Some(e), .. } => expr_is_pure(e, known),
            Stmt::Let { init: None, .. } => true,
            Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => expr_is_pure(e, known),
            Stmt::Return(None) => true,
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                expr_is_pure(condition, known)
                    && then_branch.iter().all(|s| stmt_is_pure(s, known))
                    && else_branch
                        .as_ref()
                        .is_none_or(|b| b.iter().all(|s| stmt_is_pure(s, known)))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                expr_is_pure(condition, known) && body.iter().all(|s| stmt_is_pure(s, known))
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_none_or(|i| stmt_is_pure(i, known))
                    && condition.as_ref().is_none_or(|c| expr_is_pure(c, known))
                    && update.as_ref().is_none_or(|u| expr_is_pure(u, known))
                    && body.iter().all(|s| stmt_is_pure(s, known))
            }
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
            _ => false, // conservative: reject Switch, Try, etc.
        }
    }

    func.body.iter().all(|s| stmt_is_pure(s, &known_ids))
}

/// Check if statements have simple control flow suitable for inlining
fn has_simple_control_flow(stmts: &[Stmt]) -> bool {
    for stmt in stmts {
        match stmt {
            // `Stmt::Throw` is allowed: an inlined body that throws just
            // raises the same exception in the caller's frame, which is
            // the correct propagation semantic for JS. Most ECS code
            // hot-paths through `private assert*` helpers shaped as
            // `if (!cond) { throw new Error(...) }` — without inlining,
            // the assertion is an unconditional cross-module dispatch
            // per call.
            Stmt::Let { .. } | Stmt::Expr(_) | Stmt::Return(_) | Stmt::Throw(_) => {}
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                if !has_simple_control_flow(then_branch) {
                    return false;
                }
                if let Some(else_b) = else_branch {
                    if !has_simple_control_flow(else_b) {
                        return false;
                    }
                }
            }
            Stmt::While { .. }
            | Stmt::DoWhile { .. }
            | Stmt::For { .. }
            | Stmt::Try { .. }
            | Stmt::Switch { .. }
            | Stmt::Labeled { .. }
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_) => {
                return false;
            }
            Stmt::PreallocateBoxes(_) => {}
        }
    }
    true
}

/// Find the maximum local ID used in statements
fn find_max_local_id(stmts: &[Stmt]) -> LocalId {
    let mut max_id: LocalId = 0;

    // Track every LocalId encountered. Per-variant handling for the LocalId
    // fields owned directly by an Expr; descent into sub-expressions is
    // delegated to `walk_expr_children` (single source of truth — see
    // `perry_hir::walker` for why). Pre-refactor this fn carried its own
    // ad-hoc walker with a `_ => {}` catch-all, which silently undercounted
    // any new LocalId-bearing variant (issues #167, #169, #214).
    fn check_expr(expr: &Expr, max_id: &mut LocalId) {
        match expr {
            Expr::LocalGet(id) | Expr::LocalSet(id, _) => {
                *max_id = (*max_id).max(*id);
            }
            Expr::Update { id, .. } => {
                *max_id = (*max_id).max(*id);
            }
            Expr::ArrayPush { array_id, .. }
            | Expr::ArrayPushSpread { array_id, .. }
            | Expr::ArrayUnshift { array_id, .. }
            | Expr::ArraySplice { array_id, .. }
            | Expr::ArrayCopyWithin { array_id, .. } => {
                *max_id = (*max_id).max(*array_id);
            }
            Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
                *max_id = (*max_id).max(*id);
            }
            Expr::SetAdd { set_id, .. } => {
                *max_id = (*max_id).max(*set_id);
            }
            Expr::Closure {
                params,
                body,
                captures,
                mutable_captures,
                ..
            } => {
                // Closure has THREE LocalId sources: params, captures,
                // mutable_captures. The body's nested LocalGets contribute via
                // check_stmt. Param defaults need check_expr too. Short-circuit
                // (`return`) so the walker below doesn't double-descend into
                // Param defaults.
                for param in params {
                    *max_id = (*max_id).max(param.id);
                    if let Some(d) = &param.default {
                        check_expr(d, max_id);
                    }
                }
                for id in captures {
                    *max_id = (*max_id).max(*id);
                }
                for id in mutable_captures {
                    *max_id = (*max_id).max(*id);
                }
                for stmt in body {
                    check_stmt(stmt, max_id);
                }
                return;
            }
            _ => {}
        }
        // Descend into all immediate sub-expressions. Exhaustive on Expr —
        // a new variant added to ir.rs without updating walker.rs is a
        // compile error.
        walk_expr_children(expr, &mut |child| check_expr(child, max_id));
    }

    fn check_stmt(stmt: &Stmt, max_id: &mut LocalId) {
        match stmt {
            Stmt::Let { id, init, .. } => {
                *max_id = (*max_id).max(*id);
                if let Some(expr) = init {
                    check_expr(expr, max_id);
                }
            }
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                check_expr(expr, max_id);
            }
            Stmt::Return(None) => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition, max_id);
                for s in then_branch {
                    check_stmt(s, max_id);
                }
                if let Some(else_b) = else_branch {
                    for s in else_b {
                        check_stmt(s, max_id);
                    }
                }
            }
            Stmt::While { condition, body } => {
                check_expr(condition, max_id);
                for s in body {
                    check_stmt(s, max_id);
                }
            }
            Stmt::DoWhile { body, condition } => {
                for s in body {
                    check_stmt(s, max_id);
                }
                check_expr(condition, max_id);
            }
            Stmt::Labeled { body, .. } => {
                check_stmt(body, max_id);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    check_stmt(i, max_id);
                }
                if let Some(c) = condition {
                    check_expr(c, max_id);
                }
                if let Some(u) = update {
                    check_expr(u, max_id);
                }
                for s in body {
                    check_stmt(s, max_id);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    check_stmt(s, max_id);
                }
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        *max_id = (*max_id).max(*id);
                    }
                    for s in &c.body {
                        check_stmt(s, max_id);
                    }
                }
                if let Some(f) = finally {
                    for s in f {
                        check_stmt(s, max_id);
                    }
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                check_expr(discriminant, max_id);
                for case in cases {
                    if let Some(test) = &case.test {
                        check_expr(test, max_id);
                    }
                    for s in &case.body {
                        check_stmt(s, max_id);
                    }
                }
            }
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
            Stmt::PreallocateBoxes(ids) => {
                for id in ids {
                    *max_id = (*max_id).max(*id);
                }
            }
        }
    }

    for stmt in stmts {
        check_stmt(stmt, &mut max_id);
    }

    max_id
}

/// Resolve the static class of a method-call receiver expression. Supports
/// the three shapes the inliner needs:
///   - `Expr::LocalGet(id)` whose static type is recorded in `local_types`
///   - `Expr::This` (only when we're inside a class method, supplied via
///     `enclosing_class`)
///   - `Expr::PropertyGet { object, property }` resolved recursively, then
///     the inner class's `(class, field)` looked up in `class_field_types`
///
/// Returns `(class_name, Some(local_id))` when the receiver bottoms out at a
/// `LocalGet`, so the existing `substitute_this` rewrite can target the right
/// local. For `This`-rooted chains we return `None` for the local id — the
/// inlined body's `Expr::This` already refers to the same `this` the caller's
/// context provides.
#[allow(dead_code)]
fn resolve_receiver_class(
    obj: &Expr,
    local_types: &HashMap<LocalId, String>,
    enclosing_class: Option<&str>,
    class_field_types: &HashMap<(String, String), String>,
) -> Option<(String, Option<LocalId>)> {
    match obj {
        Expr::LocalGet(id) => local_types.get(id).map(|cn| (cn.clone(), Some(*id))),
        Expr::This => enclosing_class.map(|cn| (cn.to_string(), None)),
        Expr::PropertyGet { object, property } => {
            // Recursive resolution: get the inner receiver's class, then
            // look up the field on that class. Field-walking chains like
            // `world.commandBuffer.set(...)` benefit — without this the
            // inliner's receiver match bails at the first non-LocalGet.
            let (inner_class, _) =
                resolve_receiver_class(object, local_types, enclosing_class, class_field_types)?;
            class_field_types
                .get(&(inner_class, property.clone()))
                .cloned()
                .map(|cn| (cn, None))
        }
        _ => None,
    }
}

/// True if `s` (or any nested branch) contains a `Stmt::Return`. Used by the
/// Stmt::Let-init-Call inliner to decide between the "trailing-only" collapse
/// and the do-while(false) wrapper.
fn stmt_contains_return(s: &Stmt) -> bool {
    match s {
        Stmt::Return(_) => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            then_branch.iter().any(stmt_contains_return)
                || else_branch
                    .as_ref()
                    .is_some_and(|eb| eb.iter().any(stmt_contains_return))
        }
        Stmt::Switch { cases, .. } => cases
            .iter()
            .any(|c| c.body.iter().any(stmt_contains_return)),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(stmt_contains_return)
                || catch
                    .as_ref()
                    .is_some_and(|c| c.body.iter().any(stmt_contains_return))
                || finally
                    .as_ref()
                    .is_some_and(|f| f.iter().any(stmt_contains_return))
        }
        Stmt::Labeled { body, .. } => stmt_contains_return(body.as_ref()),
        // Loops: an inner Return would terminate the OUTER function so we
        // still need to convert; but the do-while(false) wrapping handles it
        // because we descend through the loop body too. Loops don't appear in
        // is_inlinable bodies (has_simple_control_flow rejects them) so this
        // is mainly defensive.
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            body.iter().any(stmt_contains_return)
        }
        Stmt::For { body, .. } => body.iter().any(stmt_contains_return),
        _ => false,
    }
}

/// Replace every `Stmt::Return(Some(e))` in `stmts` (recursively) with
/// `Stmt::Expr(LocalSet(let_id, e)); Stmt::Break`, and every
/// `Stmt::Return(None)` with a single `Stmt::Break`. Used to convert the body
/// of an inlined function into the body of a synthetic `do { ... } while
/// (false)` wrapper at a Let-binding call site, so the value flowing through
/// `return` ends up bound to the original `let` variable.
///
/// Does NOT descend into loop bodies or `Stmt::Labeled` (those would
/// short-circuit via the inner `break` instead of breaking out of the
/// synthetic do-while). This is fine because `is_inlinable` rejects functions
/// with loops or labeled stmts, so an inlinable callee body has no such
/// structures.
fn convert_returns_in_stmts(stmts: &mut Vec<Stmt>, let_id: LocalId) {
    let mut i = 0;
    while i < stmts.len() {
        match &mut stmts[i] {
            Stmt::Return(opt) => {
                let break_stmt = Stmt::Break;
                if let Some(e) = opt.take() {
                    let assign = Stmt::Expr(Expr::LocalSet(let_id, Box::new(e)));
                    stmts[i] = assign;
                    stmts.insert(i + 1, break_stmt);
                    i += 2;
                    continue;
                } else {
                    stmts[i] = break_stmt;
                    i += 1;
                    continue;
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                convert_returns_in_stmts(then_branch, let_id);
                if let Some(eb) = else_branch {
                    convert_returns_in_stmts(eb, let_id);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    convert_returns_in_stmts(&mut c.body, let_id);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                convert_returns_in_stmts(body, let_id);
                if let Some(c) = catch {
                    convert_returns_in_stmts(&mut c.body, let_id);
                }
                if let Some(f) = finally {
                    convert_returns_in_stmts(f, let_id);
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// Inline function and method calls in a list of statements.
///
/// `enclosing_class`, when set, names the class whose method body these stmts
/// belong to. It enables inlining of `this.someMethod()` calls — without it
/// such calls fall through to runtime dispatch because the inliner only
/// recognizes `Expr::LocalGet(obj_id)` as a method receiver. The Phase 6
/// driver passes the class name; Phases 4 (init) and 5 (top-level functions)
/// pass `None`.
fn inline_calls_in_stmts(
    stmts: &mut Vec<Stmt>,
    func_candidates: &HashMap<FuncId, Function>,
    method_candidates: &HashMap<(String, String), MethodCandidate>,
    class_names: &HashMap<String, String>,
    local_types: &mut HashMap<LocalId, String>,
    next_local_id: &mut LocalId,
    enclosing_class: Option<&str>,
    class_field_types: &HashMap<(String, String), String>,
) {
    let mut i = 0;
    while i < stmts.len() {
        // Track local variable types from Let statements
        if let Stmt::Let { id, ty, init, .. } = &stmts[i] {
            if let Type::Named(class_name) = ty {
                local_types.insert(*id, class_name.clone());
            }
            // Also check if init is a New expression
            if let Some(Expr::New { class_name, .. }) = init {
                local_types.insert(*id, class_name.clone());
            }
        }

        let mut new_stmts: Option<Vec<Stmt>> = None;

        match &mut stmts[i] {
            Stmt::Expr(expr) => {
                if let Some((mut inlined_stmts, _result_expr)) = try_inline_call(
                    expr,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                ) {
                    // When inlining into Stmt::Expr context (result discarded),
                    // convert Stmt::Return(Some(expr)) to Stmt::Expr(expr) and
                    // remove Stmt::Return(None). This prevents emitting a
                    // `ret` terminator mid-block (e.g., inside a for loop body).
                    // Only do this if returns are in safe positions (trailing).
                    let has_nested_return = inlined_stmts
                        .iter()
                        .take(inlined_stmts.len().saturating_sub(1))
                        .any(|s| {
                            fn stmt_has_return(s: &Stmt) -> bool {
                                match s {
                                    Stmt::Return(_) => true,
                                    Stmt::If {
                                        then_branch,
                                        else_branch,
                                        ..
                                    } => {
                                        then_branch.iter().any(stmt_has_return)
                                            || else_branch
                                                .as_ref()
                                                .is_some_and(|eb| eb.iter().any(stmt_has_return))
                                    }
                                    _ => false,
                                }
                            }
                            stmt_has_return(s)
                        });
                    if has_nested_return {
                        // Can't safely convert early returns; skip inlining
                        let hoisted = inline_calls_in_expr(
                            expr,
                            func_candidates,
                            method_candidates,
                            local_types,
                            next_local_id,
                            enclosing_class,
                            class_field_types,
                        );
                        if !hoisted.is_empty() {
                            new_stmts = Some(hoisted);
                        }
                    } else {
                        // Convert trailing return to expression (discard result)
                        if let Some(last) = inlined_stmts.last_mut() {
                            match last {
                                Stmt::Return(Some(ret_expr)) => {
                                    let e = std::mem::replace(ret_expr, Expr::Undefined);
                                    *last = Stmt::Expr(e);
                                }
                                Stmt::Return(None) => {
                                    inlined_stmts.pop();
                                }
                                _ => {}
                            }
                        }
                        new_stmts = Some(inlined_stmts);
                    }
                } else {
                    let hoisted = inline_calls_in_expr(
                        expr,
                        func_candidates,
                        method_candidates,
                        local_types,
                        next_local_id,
                        enclosing_class,
                        class_field_types,
                    );
                    if !hoisted.is_empty() {
                        // Hoisted stmts from multi-stmt inlining inside expressions
                        // (e.g., `h = imul32(h, p)` → Let setup stmts + modified expr)
                        // Splice them before the current statement, keeping the stmt itself.
                        let current = stmts.remove(i);
                        let hoisted_len = hoisted.len();
                        for (j, s) in hoisted.into_iter().enumerate() {
                            stmts.insert(i + j, s);
                        }
                        stmts.insert(i + hoisted_len, current);
                        i += hoisted_len + 1;
                        continue;
                    }
                }
            }
            Stmt::Let { init: Some(_), .. } => {
                // First try inlining a top-level Call as the Let's init.
                // Pattern:    `let r = f(args)`  or  `let r = this.m(args)`
                // becomes:    `let r = undefined; do { /* inlined body, with
                //               every Return(Some(e)) replaced by
                //               Expr(LocalSet(r, e)); Break */ } while (false)`
                // The wrapper is needed because the inlined body may have early
                // returns inside `if` branches; converting them all uniformly to
                // LocalSet+Break preserves semantics. After this rewrite the
                // returned object literal (e.g. `{entityId, componentType,
                // component}` from `World.resolveSetOperation`) lives in the
                // caller's stmt list as a non-escaping `Let r = undefined;
                // LocalSet r = { ... }`, where the existing
                // `non_escaping_object_literals` collector can then scalar-
                // replace it during codegen.
                //
                // For the simple trailing-only case (no nested returns), we
                // collapse the wrapper: the inlined setup stmts run inline,
                // then the trailing `Return(Some(e))` becomes the original
                // `Let { id: let_id, init: Some(e) }`.
                let (let_id, let_name, let_ty, let_mutable) = match &stmts[i] {
                    Stmt::Let {
                        id,
                        name,
                        ty,
                        mutable,
                        ..
                    } => (*id, name.clone(), ty.clone(), *mutable),
                    _ => unreachable!(),
                };
                let init_expr = match &stmts[i] {
                    Stmt::Let { init: Some(e), .. } => e.clone(),
                    _ => unreachable!(),
                };
                let mut handled = false;
                if matches!(&init_expr, Expr::Call { .. }) {
                    if let Some((mut inlined_stmts, _)) = try_inline_call(
                        &init_expr,
                        func_candidates,
                        method_candidates,
                        local_types,
                        next_local_id,
                        enclosing_class,
                        class_field_types,
                    ) {
                        let has_nested_return = inlined_stmts
                            .iter()
                            .take(inlined_stmts.len().saturating_sub(1))
                            .any(stmt_contains_return);
                        let trailing_is_return =
                            matches!(inlined_stmts.last(), Some(Stmt::Return(Some(_))));
                        if !has_nested_return && trailing_is_return {
                            // Collapse: convert the trailing Return into the
                            // original Let-binding.
                            if let Some(last) = inlined_stmts.last_mut() {
                                if let Stmt::Return(Some(ret_expr)) = last {
                                    let e = std::mem::replace(ret_expr, Expr::Undefined);
                                    *last = Stmt::Let {
                                        id: let_id,
                                        name: let_name.clone(),
                                        ty: let_ty.clone(),
                                        mutable: let_mutable,
                                        init: Some(e),
                                    };
                                }
                            }
                            new_stmts = Some(inlined_stmts);
                            handled = true;
                        } else if inlined_stmts.iter().any(stmt_contains_return) {
                            // Nested-return case: wrap in `do { ... } while (false)`,
                            // converting every Return(Some(e)) to LocalSet+Break and
                            // every Return(None) to Break. The original Let becomes
                            // a mutable seed initialized to undefined; the wrapper
                            // body then writes the result via LocalSet.
                            convert_returns_in_stmts(&mut inlined_stmts, let_id);
                            let mut wrapped: Vec<Stmt> = Vec::with_capacity(2);
                            wrapped.push(Stmt::Let {
                                id: let_id,
                                name: let_name.clone(),
                                ty: let_ty.clone(),
                                // Force mutable — even though the source was a
                                // const, we now write to it via LocalSet.
                                mutable: true,
                                init: Some(Expr::Undefined),
                            });
                            wrapped.push(Stmt::DoWhile {
                                body: inlined_stmts,
                                condition: Expr::Bool(false),
                            });
                            new_stmts = Some(wrapped);
                            handled = true;
                        }
                    }
                }
                if !handled {
                    // Fall back to nested-arg hoisting (existing behavior).
                    let hoisted = match &mut stmts[i] {
                        Stmt::Let {
                            init: Some(expr), ..
                        } => inline_calls_in_expr(
                            expr,
                            func_candidates,
                            method_candidates,
                            local_types,
                            next_local_id,
                            enclosing_class,
                            class_field_types,
                        ),
                        _ => Vec::new(),
                    };
                    if !hoisted.is_empty() {
                        let current = stmts.remove(i);
                        let hoisted_len = hoisted.len();
                        for (j, s) in hoisted.into_iter().enumerate() {
                            stmts.insert(i + j, s);
                        }
                        stmts.insert(i + hoisted_len, current);
                        i += hoisted_len + 1;
                        continue;
                    }
                }
            }
            Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                let hoisted = inline_calls_in_expr(
                    expr,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                );
                if !hoisted.is_empty() {
                    let current = stmts.remove(i);
                    let hoisted_len = hoisted.len();
                    for (j, s) in hoisted.into_iter().enumerate() {
                        stmts.insert(i + j, s);
                    }
                    stmts.insert(i + hoisted_len, current);
                    i += hoisted_len + 1;
                    continue;
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let _hoisted = inline_calls_in_expr(
                    condition,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                );
                // Note: hoisting from conditions is rare and complex; skip for now
                inline_calls_in_stmts(
                    then_branch,
                    func_candidates,
                    method_candidates,
                    class_names,
                    local_types,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                );
                if let Some(else_b) = else_branch {
                    inline_calls_in_stmts(
                        else_b,
                        func_candidates,
                        method_candidates,
                        class_names,
                        local_types,
                        next_local_id,
                        enclosing_class,
                        class_field_types,
                    );
                }
            }
            Stmt::While { condition, body } => {
                let _hoisted = inline_calls_in_expr(
                    condition,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                );
                inline_calls_in_stmts(
                    body,
                    func_candidates,
                    method_candidates,
                    class_names,
                    local_types,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                );
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    let mut init_stmts = vec![*init_stmt.clone()];
                    inline_calls_in_stmts(
                        &mut init_stmts,
                        func_candidates,
                        method_candidates,
                        class_names,
                        local_types,
                        next_local_id,
                        enclosing_class,
                        class_field_types,
                    );
                    if init_stmts.len() == 1 {
                        **init_stmt = init_stmts.remove(0);
                    }
                }
                if let Some(cond) = condition {
                    let _hoisted = inline_calls_in_expr(
                        cond,
                        func_candidates,
                        method_candidates,
                        local_types,
                        next_local_id,
                        enclosing_class,
                        class_field_types,
                    );
                }
                if let Some(upd) = update {
                    let _hoisted = inline_calls_in_expr(
                        upd,
                        func_candidates,
                        method_candidates,
                        local_types,
                        next_local_id,
                        enclosing_class,
                        class_field_types,
                    );
                }
                inline_calls_in_stmts(
                    body,
                    func_candidates,
                    method_candidates,
                    class_names,
                    local_types,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                );
            }
            _ => {}
        }

        if let Some(mut inlined) = new_stmts {
            // Recursively inline calls within the just-inlined block before
            // splicing it back. Without this, the body of an inlined method
            // (e.g. `World.set` once it's expanded inline) would itself contain
            // un-inlined calls — like `this.resolveSetOperation(...)` — that
            // get skipped because the outer iterator advances past their
            // positions. Doing the inner pass first means subsequent layers of
            // small-function calls collapse cleanly: `world.set(e, C, v)` →
            // `this.resolveSetOperation(...) + this.commandBuffer.set(...)` →
            // scalar-replaced `{entityId, componentType, component}` literal.
            //
            // Termination relies on `is_inlinable` rejecting recursive
            // functions in practice (the static-call analysis here only
            // matches when both the receiver class and method are statically
            // known, so cyclic call chains either don't form or get filtered
            // out by other criteria). Phase 6's "skip if itself a candidate"
            // gate (line ~190) already prevents the most direct case.
            inline_calls_in_stmts(
                &mut inlined,
                func_candidates,
                method_candidates,
                class_names,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            );
            stmts.remove(i);
            let inlined_len = inlined.len();
            for (j, stmt) in inlined.drain(..).enumerate() {
                stmts.insert(i + j, stmt);
            }
            i += inlined_len.max(1);
        } else {
            i += 1;
        }
    }
}

/// Inline function and method calls in an expression.
/// Returns setup statements that must be spliced before the enclosing statement.
fn inline_calls_in_expr(
    expr: &mut Expr,
    func_candidates: &HashMap<FuncId, Function>,
    method_candidates: &HashMap<(String, String), MethodCandidate>,
    local_types: &HashMap<LocalId, String>,
    next_local_id: &mut LocalId,
    enclosing_class: Option<&str>,
    class_field_types: &HashMap<(String, String), String>,
) -> Vec<Stmt> {
    // First try to inline this expression if it's a call
    if let Some((stmts, mut result)) = try_inline_simple_call(
        expr,
        func_candidates,
        method_candidates,
        local_types,
        next_local_id,
        enclosing_class,
        class_field_types,
    ) {
        let inner = inline_calls_in_expr(
            &mut result,
            func_candidates,
            method_candidates,
            local_types,
            next_local_id,
            enclosing_class,
            class_field_types,
        );
        *expr = result;
        let mut all = stmts;
        all.extend(inner);
        return all;
    }

    // Otherwise recurse into sub-expressions, collecting hoisted stmts
    let mut hoisted = Vec::new();
    match expr {
        Expr::Binary { left, right, .. }
        | Expr::Logical { left, right, .. }
        | Expr::Compare { left, right, .. } => {
            hoisted.extend(inline_calls_in_expr(
                left,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                right,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::Unary { operand, .. } => {
            hoisted.extend(inline_calls_in_expr(
                operand,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            hoisted.extend(inline_calls_in_expr(
                condition,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                then_expr,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                else_expr,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::Call { callee, args, .. } => {
            hoisted.extend(inline_calls_in_expr(
                callee,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            for arg in args {
                hoisted.extend(inline_calls_in_expr(
                    arg,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                ));
            }
        }
        Expr::Array(elements) => {
            for elem in elements {
                hoisted.extend(inline_calls_in_expr(
                    elem,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                ));
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                hoisted.extend(inline_calls_in_expr(
                    v,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                ));
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts {
                hoisted.extend(inline_calls_in_expr(
                    v,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                ));
            }
        }
        Expr::ArraySpread(elements) => {
            for elem in elements {
                match elem {
                    perry_hir::ArrayElement::Expr(e) | perry_hir::ArrayElement::Spread(e) => {
                        hoisted.extend(inline_calls_in_expr(
                            e,
                            func_candidates,
                            method_candidates,
                            local_types,
                            next_local_id,
                            enclosing_class,
                            class_field_types,
                        ));
                    }
                }
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            hoisted.extend(inline_calls_in_expr(
                callee,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            for arg in args {
                match arg {
                    perry_hir::CallArg::Expr(e) | perry_hir::CallArg::Spread(e) => {
                        hoisted.extend(inline_calls_in_expr(
                            e,
                            func_candidates,
                            method_candidates,
                            local_types,
                            next_local_id,
                            enclosing_class,
                            class_field_types,
                        ));
                    }
                }
            }
        }
        Expr::IndexGet { object, index } => {
            hoisted.extend(inline_calls_in_expr(
                object,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                index,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            hoisted.extend(inline_calls_in_expr(
                object,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                index,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                value,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::PropertyGet { object, .. } => {
            hoisted.extend(inline_calls_in_expr(
                object,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::PropertySet { object, value, .. } => {
            hoisted.extend(inline_calls_in_expr(
                object,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                value,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::LocalSet(_, value) => {
            hoisted.extend(inline_calls_in_expr(
                value,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                hoisted.extend(inline_calls_in_expr(
                    obj,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                ));
            }
            for arg in args {
                hoisted.extend(inline_calls_in_expr(
                    arg,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                ));
            }
        }
        // Issue #169: a Call nested inside a Uint8Array index/set/length
        // (e.g. `buf[clamp(i)]`) wouldn't be inlined without these arms.
        Expr::Uint8ArrayGet { array, index } => {
            hoisted.extend(inline_calls_in_expr(
                array,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                index,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => {
            hoisted.extend(inline_calls_in_expr(
                array,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                index,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                value,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::Uint8ArrayLength(arr) => {
            hoisted.extend(inline_calls_in_expr(
                arr,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::Uint8ArrayNew(Some(arg)) => {
            hoisted.extend(inline_calls_in_expr(
                arg,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        // Descend into closure bodies. Without this, the inliner never
        // visits the body of an arrow/`function(){}` literal — which
        // is a significant gap because test fixtures wrap their entire
        // workload in `describe(() => it(() => { ... }))` callbacks, and
        // the test loop's hot calls (e.g. `world.set(...)`) live exclusively
        // inside those nested closures. Use the closure's HIR-recorded
        // `enclosing_class` (Some(class) iff the closure captures `this`
        // from a class method) so calls of the form `this.method(...)`
        // inside an arrow inside a class method still resolve correctly.
        // Default-param expressions are NOT descended into here — they
        // execute in the closure's context but are evaluated at the call
        // site of the closure, where param types may differ.
        Expr::Closure {
            body,
            params,
            enclosing_class: closure_enclosing,
            ..
        } => {
            // Seed local_types entries for each param with a Named class
            // type, so calls of the form `paramName.method(...)` inside the
            // closure can still resolve via the LocalGet path.
            let mut closure_local_types = local_types.clone();
            for p in params.iter() {
                if let Type::Named(class_name) = &p.ty {
                    closure_local_types.insert(p.id, class_name.clone());
                }
            }
            // Hoist any setup stmts produced by inlining inside the body
            // up to the call-site context. For closures these typically
            // would be empty, but stay defensive.
            inline_calls_in_stmts(
                body,
                func_candidates,
                method_candidates,
                &HashMap::new(),
                &mut closure_local_types,
                next_local_id,
                closure_enclosing.as_deref(),
                class_field_types,
            );
        }
        _ => {}
    }
    hoisted
}

/// Try to inline a simple function or method call.
/// Handles two patterns:
/// 1. Single `Return(expr)` body — classic expression-level inline
/// 2. `[Let*, Return(expr)]` body — setup stmts + result expression
fn try_inline_simple_call(
    expr: &Expr,
    func_candidates: &HashMap<FuncId, Function>,
    method_candidates: &HashMap<(String, String), MethodCandidate>,
    local_types: &HashMap<LocalId, String>,
    next_local_id: &mut LocalId,
    enclosing_class: Option<&str>,
    class_field_types: &HashMap<(String, String), String>,
) -> Option<(Vec<Stmt>, Expr)> {
    if let Expr::Call { callee, args, .. } = expr {
        // Check for regular function call
        if let Expr::FuncRef(func_id) = callee.as_ref() {
            if let Some(func) = func_candidates.get(func_id) {
                // Pattern 1: single Return(expr)
                if func.body.len() == 1 {
                    if let Stmt::Return(Some(return_expr)) = &func.body[0] {
                        let mut param_map: HashMap<LocalId, Expr> = HashMap::new();
                        for (param, arg) in func.params.iter().zip(args.iter()) {
                            param_map.insert(param.id, arg.clone());
                        }
                        let mut result = return_expr.clone();
                        substitute_locals(&mut result, &param_map, next_local_id);
                        return Some((vec![], result));
                    }
                }

                // Pattern 2: [Let (const)*, Return(expr)] — e.g. imul32 polyfill
                // All statements except the last must be immutable Let declarations,
                // and the last must be Return(Some(expr)).
                if func.body.len() > 1 {
                    let last = func.body.last().unwrap();
                    if let Stmt::Return(Some(return_expr)) = last {
                        let all_lets = func.body[..func.body.len() - 1].iter().all(|s| {
                            matches!(
                                s,
                                Stmt::Let {
                                    mutable: false,
                                    init: Some(_),
                                    ..
                                }
                            )
                        });
                        if all_lets {
                            // Build param substitution map
                            let mut param_map: HashMap<LocalId, Expr> = HashMap::new();
                            for (param, arg) in func.params.iter().zip(args.iter()) {
                                if is_trivial_expr(arg) {
                                    param_map.insert(param.id, arg.clone());
                                } else {
                                    let fresh = *next_local_id;
                                    *next_local_id += 1;
                                    param_map.insert(param.id, Expr::LocalGet(fresh));
                                    // We'll create the Let for this fresh id below
                                }
                            }

                            // Remap body-local IDs
                            let body_ids = collect_body_local_ids(&func.body);
                            for old_id in &body_ids {
                                if !param_map.contains_key(old_id) {
                                    let fresh = *next_local_id;
                                    *next_local_id += 1;
                                    param_map.insert(*old_id, Expr::LocalGet(fresh));
                                }
                            }

                            // Build setup stmts: param Lets (for non-trivial args) + body Lets
                            let mut setup: Vec<Stmt> = Vec::new();

                            // First, add Lets for non-trivial param args
                            for (param, arg) in func.params.iter().zip(args.iter()) {
                                if !is_trivial_expr(arg) {
                                    if let Some(Expr::LocalGet(fresh_id)) = param_map.get(&param.id)
                                    {
                                        setup.push(Stmt::Let {
                                            id: *fresh_id,
                                            name: param.name.clone(),
                                            ty: param.ty.clone(),
                                            mutable: false,
                                            init: Some(arg.clone()),
                                        });
                                    }
                                }
                            }

                            // Then clone the body Let stmts with substituted inits
                            for stmt in &func.body[..func.body.len() - 1] {
                                if let Stmt::Let {
                                    id,
                                    name,
                                    ty,
                                    mutable,
                                    init: Some(init_expr),
                                } = stmt
                                {
                                    let new_id =
                                        if let Some(Expr::LocalGet(fresh)) = param_map.get(id) {
                                            *fresh
                                        } else {
                                            *id
                                        };
                                    let mut new_init = init_expr.clone();
                                    substitute_locals(&mut new_init, &param_map, next_local_id);
                                    setup.push(Stmt::Let {
                                        id: new_id,
                                        name: name.clone(),
                                        ty: ty.clone(),
                                        mutable: *mutable,
                                        init: Some(new_init),
                                    });
                                }
                            }

                            // Build result expression from the Return
                            let mut result = return_expr.clone();
                            substitute_locals(&mut result, &param_map, next_local_id);

                            return Some((setup, result));
                        }
                    }
                }
            }
        }

        // Check for method call: callee is PropertyGet { object: LocalGet(id), property: method_name }
        if let Expr::PropertyGet {
            object,
            property: method_name,
        } = callee.as_ref()
        {
            // Resolve the receiver class via `resolve_receiver_class`:
            //   - `Expr::LocalGet(id)` whose static type is a known class:
            //     LocalGet substitution; `Expr::This` in the body rewrites to
            //     `Expr::LocalGet(id)`.
            //   - `Expr::This`, when we're inside a class method of the same
            //     class: `Expr::This` in the body stays `Expr::This` (it
            //     refers to the same `this` as the caller's context).
            //   - `Expr::PropertyGet { ... }` chain (e.g. `world.commandBuffer`):
            //     resolved by walking the chain via `class_field_types`. The
            //     simple-call path here can't materialize a setup-Let for the
            //     receiver (it must produce a single Expr result with an
            //     empty stmt list for the single-Return case), so we only
            //     accept LocalGet/This shapes here. The richer
            //     `try_inline_call` path (multi-stmt) handles PropertyGet
            //     receivers by emitting a `Let __recv = <chain>` first.
            let receiver: Option<(String, Option<LocalId>)> = match object.as_ref() {
                Expr::LocalGet(obj_id) => local_types
                    .get(obj_id)
                    .map(|cn| (cn.clone(), Some(*obj_id))),
                Expr::This => enclosing_class.map(|cn| (cn.to_string(), None)),
                _ => None,
            };
            // Silence unused-warning for the resolver helper when this
            // simple path doesn't reach it. The richer path below uses it.
            let _ = class_field_types;
            if let Some((class_name, obj_id_opt)) = receiver {
                // Look up the method candidate
                if let Some(method_candidate) =
                    method_candidates.get(&(class_name, method_name.clone()))
                {
                    // Check for single return statement
                    if method_candidate.func.body.len() == 1 {
                        if let Stmt::Return(Some(return_expr)) = &method_candidate.func.body[0] {
                            let mut param_map: HashMap<LocalId, Expr> = HashMap::new();

                            // Map 'this' parameter to the receiver object (only
                            // when the receiver is a LocalGet — for an
                            // `Expr::This` receiver the body's `Expr::This`
                            // already references the same `this` as the
                            // caller, so we leave it alone).
                            if let (Some(this_id), Some(obj_id)) =
                                (method_candidate.this_param_id, obj_id_opt)
                            {
                                param_map.insert(this_id, Expr::LocalGet(obj_id));
                            }

                            // Map parameters to arguments
                            // Note: Method params don't include 'this' - they use Expr::This instead
                            for (param, arg) in method_candidate.func.params.iter().zip(args.iter())
                            {
                                param_map.insert(param.id, arg.clone());
                            }

                            let mut result = return_expr.clone();
                            substitute_locals(&mut result, &param_map, next_local_id);

                            // Also substitute Expr::This with the receiver
                            // (only when the receiver was a LocalGet).
                            if let Some(obj_id) = obj_id_opt {
                                substitute_this(&mut result, obj_id);
                            }

                            return Some((vec![], result));
                        }
                    }

                    // Handle void methods (no return or empty return)
                    if method_candidate.func.body.len() <= 2 {
                        let mut is_void_method = true;
                        let mut inlined_stmts = Vec::new();

                        for stmt in &method_candidate.func.body {
                            match stmt {
                                Stmt::Return(None) => {}
                                Stmt::Expr(e) => {
                                    let mut param_map: HashMap<LocalId, Expr> = HashMap::new();
                                    if let (Some(this_id), Some(obj_id)) =
                                        (method_candidate.this_param_id, obj_id_opt)
                                    {
                                        param_map.insert(this_id, Expr::LocalGet(obj_id));
                                    }
                                    // Note: Method params don't include 'this' - they use Expr::This instead
                                    for (param, arg) in
                                        method_candidate.func.params.iter().zip(args.iter())
                                    {
                                        param_map.insert(param.id, arg.clone());
                                    }
                                    let mut expr = e.clone();
                                    substitute_locals(&mut expr, &param_map, next_local_id);
                                    if let Some(obj_id) = obj_id_opt {
                                        substitute_this(&mut expr, obj_id);
                                    }
                                    inlined_stmts.push(Stmt::Expr(expr));
                                }
                                _ => {
                                    is_void_method = false;
                                    break;
                                }
                            }
                        }

                        if is_void_method && !inlined_stmts.is_empty() {
                            return Some((inlined_stmts, Expr::Undefined));
                        }
                    }
                }
            }
        }
    }
    None
}

/// Try to inline a call that may have multiple statements
fn try_inline_call(
    expr: &Expr,
    func_candidates: &HashMap<FuncId, Function>,
    method_candidates: &HashMap<(String, String), MethodCandidate>,
    local_types: &HashMap<LocalId, String>,
    next_local_id: &mut LocalId,
    enclosing_class: Option<&str>,
    class_field_types: &HashMap<(String, String), String>,
) -> Option<(Vec<Stmt>, Option<Expr>)> {
    if let Expr::Call { callee, args, .. } = expr {
        // Handle regular function calls
        if let Expr::FuncRef(func_id) = callee.as_ref() {
            if let Some(func) = func_candidates.get(func_id) {
                let mut setup_stmts: Vec<Stmt> = Vec::new();
                let mut param_map: HashMap<LocalId, Expr> = HashMap::new();

                for (param, arg) in func.params.iter().zip(args.iter()) {
                    if is_trivial_expr(arg) {
                        param_map.insert(param.id, arg.clone());
                    } else {
                        let local_id = *next_local_id;
                        *next_local_id += 1;

                        setup_stmts.push(Stmt::Let {
                            id: local_id,
                            name: param.name.clone(),
                            ty: param.ty.clone(),
                            mutable: false,
                            init: Some(arg.clone()),
                        });

                        param_map.insert(param.id, Expr::LocalGet(local_id));
                    }
                }

                // Trailing optional/default params with no matching arg —
                // see the matching block in the method-call branch below for
                // the rationale. Without this, references to the unmatched
                // param's source-side LocalId leak into the destination.
                for param in func.params.iter().skip(args.len()) {
                    let local_id = *next_local_id;
                    *next_local_id += 1;
                    setup_stmts.push(Stmt::Let {
                        id: local_id,
                        name: param.name.clone(),
                        ty: param.ty.clone(),
                        mutable: true,
                        init: Some(Expr::Undefined),
                    });
                    param_map.insert(param.id, Expr::LocalGet(local_id));
                }

                let mut inlined_body = func.body.clone();

                // Collect all LocalIds from Let statements in the body and remap them
                let body_local_ids = collect_body_local_ids(&inlined_body);
                for old_id in body_local_ids {
                    param_map.entry(old_id).or_insert_with(|| {
                        let new_id = *next_local_id;
                        *next_local_id += 1;
                        Expr::LocalGet(new_id)
                    });
                }

                substitute_locals_in_stmts(&mut inlined_body, &param_map, next_local_id);

                setup_stmts.extend(inlined_body);

                return Some((setup_stmts, None));
            }
        }

        // Handle method calls
        if let Expr::PropertyGet {
            object,
            property: method_name,
        } = callee.as_ref()
        {
            // Resolve receiver class. Three shapes are supported:
            //   - `Expr::LocalGet(id)` whose static type is in `local_types`
            //     (existing case, no setup needed).
            //   - `Expr::This` (existing case, `Expr::This` in the body stays
            //     unchanged because the caller's `this` already matches when
            //     `enclosing_class` is set).
            //   - Any other shape that `resolve_receiver_class` can resolve
            //     (notably `PropertyGet { object: LocalGet(id), property: f }`
            //     when `class_field_types` knows `(class_of_id, f) ->
            //     field_class`). For these we materialize the receiver into
            //     a fresh local via a setup `Let` and then drive
            //     `substitute_this_in_stmts` against that local — without
            //     materialization the body's `Expr::This` would reference the
            //     CALLER's `this`, which is the wrong receiver class.
            let mut setup_stmts: Vec<Stmt> = Vec::new();
            // Receiver resolution: only LocalGet/This receivers are inlined.
            // PropertyGet chains (e.g. `world.commandBuffer.set(...)`) are
            // technically resolvable via `class_field_types`, but when we
            // tried inlining them by materializing the chain into a fresh
            // `Let __recv = world.commandBuffer` followed by a substituted
            // body, sync-hotpath regressed from 57 → 86 ms and the whole
            // perf-comprehensive table widened. The runtime
            // js_native_call_method dispatch (with the IC) ends up cheaper
            // at scale than the emitted alloca + store + load + inlined
            // body — likely because of the shadow-frame tracking the
            // `Named`-typed materialization triggers, and how LLVM ends up
            // handling the larger inlined body under register pressure.
            // Left as a follow-up; `class_field_types` is plumbed through
            // and `resolve_receiver_class` is defined so the next attempt
            // can swap in here without re-threading the signatures.
            let _ = class_field_types;
            let receiver: Option<(String, Option<LocalId>)> = match object.as_ref() {
                Expr::LocalGet(obj_id) => local_types
                    .get(obj_id)
                    .map(|cn| (cn.clone(), Some(*obj_id))),
                Expr::This => enclosing_class.map(|cn| (cn.to_string(), None)),
                _ => None,
            };
            if let Some((class_name, obj_id_opt)) = receiver {
                if let Some(method_candidate) =
                    method_candidates.get(&(class_name, method_name.clone()))
                {
                    let mut param_map: HashMap<LocalId, Expr> = HashMap::new();

                    // Map 'this' parameter to the receiver object (if present
                    // as a param AND we have a concrete obj_id). For
                    // `Expr::This` receivers there's nothing to map — the
                    // body's `Expr::This` stays as-is.
                    if let (Some(this_id), Some(obj_id)) =
                        (method_candidate.this_param_id, obj_id_opt)
                    {
                        param_map.insert(this_id, Expr::LocalGet(obj_id));
                    }

                    // Map parameters to arguments
                    // Note: Method params don't include 'this' - they use Expr::This instead
                    for (param, arg) in method_candidate.func.params.iter().zip(args.iter()) {
                        if is_trivial_expr(arg) {
                            param_map.insert(param.id, arg.clone());
                        } else {
                            let local_id = *next_local_id;
                            *next_local_id += 1;

                            setup_stmts.push(Stmt::Let {
                                id: local_id,
                                name: param.name.clone(),
                                ty: param.ty.clone(),
                                mutable: false,
                                init: Some(arg.clone()),
                            });

                            param_map.insert(param.id, Expr::LocalGet(local_id));
                        }
                    }

                    // Trailing optional/default params with no matching arg:
                    // allocate a fresh local for each so the param's source-
                    // class LocalId doesn't leak into the destination scope
                    // (where it can collide with an unrelated local — e.g.
                    // `World.createQuery(componentTypes, filter = {})` called
                    // with one arg leaves `filter`'s body refs unsubstituted,
                    // and the `if (filter === undefined) filter = {}`
                    // prologue then writes to whatever destination local
                    // happens to share that id).
                    for param in method_candidate.func.params.iter().skip(args.len()) {
                        let local_id = *next_local_id;
                        *next_local_id += 1;
                        setup_stmts.push(Stmt::Let {
                            id: local_id,
                            name: param.name.clone(),
                            ty: param.ty.clone(),
                            mutable: true,
                            init: Some(Expr::Undefined),
                        });
                        param_map.insert(param.id, Expr::LocalGet(local_id));
                    }

                    // Clone and substitute the method body
                    let mut inlined_body = method_candidate.func.body.clone();

                    // Collect all LocalIds from Let statements in the body and remap them
                    let body_local_ids = collect_body_local_ids(&inlined_body);
                    for old_id in body_local_ids {
                        param_map.entry(old_id).or_insert_with(|| {
                            let new_id = *next_local_id;
                            *next_local_id += 1;
                            Expr::LocalGet(new_id)
                        });
                    }

                    substitute_locals_in_stmts(&mut inlined_body, &param_map, next_local_id);
                    if let Some(obj_id) = obj_id_opt {
                        substitute_this_in_stmts(&mut inlined_body, obj_id);
                    }

                    setup_stmts.extend(inlined_body);

                    return Some((setup_stmts, None));
                }
            }
        }
    }
    None
}

/// Check if an expression is trivial (safe to duplicate)
fn is_trivial_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Integer(_)
            | Expr::Number(_)
            | Expr::Bool(_)
            | Expr::String(_)
            | Expr::WtfString(_)
            | Expr::Null
            | Expr::Undefined
            | Expr::LocalGet(_)
            | Expr::GlobalGet(_)
    )
}

/// Substitute local variable references in an expression
/// Replace inlined parameters' LocalGets with the actual call-site argument
/// expressions, and remap LocalIds carried by other variants when the param
/// map says so.
///
/// Per-variant work focuses on the LocalId-bearing variants (LocalGet itself
/// is the substitution target; LocalSet / Update / Array*.array_id / SetAdd /
/// Closure.captures need id-only remapping). Descent into all other
/// sub-expressions is delegated to `walk_expr_children_mut` — the central
/// exhaustive walker in `perry_hir::walker`. Pre-refactor this fn carried its
/// own ad-hoc walker with a `_ => {}` catch-all that silently dropped any new
/// variant added to `Expr` (issues #169, #214).
fn substitute_locals(
    expr: &mut Expr,
    param_map: &HashMap<LocalId, Expr>,
    next_local_id: &mut LocalId,
) {
    match expr {
        Expr::LocalGet(id) => {
            if let Some(replacement) = param_map.get(id) {
                *expr = replacement.clone();
            }
            return;
        }
        Expr::LocalSet(id, value) => {
            substitute_locals(value, param_map, next_local_id);
            if let Some(Expr::LocalGet(new_id)) = param_map.get(id) {
                *id = *new_id;
            }
            return;
        }
        Expr::Update { id, .. } => {
            if let Some(Expr::LocalGet(new_id)) = param_map.get(id) {
                *id = *new_id;
            }
            return;
        }
        Expr::ArrayPop(array_id) | Expr::ArrayShift(array_id) => {
            if let Some(Expr::LocalGet(new_id)) = param_map.get(array_id) {
                *array_id = *new_id;
            }
            return;
        }
        Expr::ArrayPush { array_id, .. }
        | Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. } => {
            if let Some(Expr::LocalGet(new_id)) = param_map.get(array_id) {
                *array_id = *new_id;
            }
            // Children (`value`, `start`, `delete_count`, `items`, `target`,
            // `end`, …) are descended into below via the walker.
        }
        Expr::SetAdd { set_id, .. } => {
            if let Some(Expr::LocalGet(new_id)) = param_map.get(set_id) {
                *set_id = *new_id;
            }
            // `value` descended via walker.
        }
        // Closure: substitute in body AND remap captures lists. Without
        // remapping captures, an inlined function whose body contains a
        // closure ends up with the closure's captures list referencing the
        // OLD local IDs while the closure body uses the NEW (remapped) IDs.
        // Codegen then can't resolve the captures in the inlined-into FnCtx
        // and falls back to `double_literal(0.0)`, producing null box
        // pointers at runtime (closure-null family). Param defaults also get
        // substituted explicitly here so the walker doesn't double-process
        // them.
        Expr::Closure {
            body,
            captures,
            mutable_captures,
            params,
            ..
        } => {
            for p in params.iter_mut() {
                if let Some(d) = &mut p.default {
                    substitute_locals(d, param_map, next_local_id);
                }
            }
            substitute_locals_in_stmts(body, param_map, next_local_id);
            captures.retain_mut(|id| match param_map.get(id) {
                Some(Expr::LocalGet(new_id)) => {
                    *id = *new_id;
                    true
                }
                // Trivial expr inlined directly; closure body no longer
                // references this id, so drop the now-orphan capture.
                Some(_) => false,
                // Not in param_map → outer/module-level; leave unchanged.
                None => true,
            });
            mutable_captures.retain_mut(|id| match param_map.get(id) {
                Some(Expr::LocalGet(new_id)) => {
                    *id = *new_id;
                    true
                }
                Some(_) => false,
                None => true,
            });
            return;
        }
        _ => {}
    }
    // Descend into all immediate sub-expressions for non-special variants.
    // The walker is exhaustive on Expr — adding a new variant to ir.rs
    // without updating walker.rs is a compile error.
    walk_expr_children_mut(expr, &mut |child| {
        substitute_locals(child, param_map, next_local_id)
    });
}

/// Substitute Expr::This with a LocalGet reference
fn substitute_this(expr: &mut Expr, obj_id: LocalId) {
    if let Expr::This = expr {
        *expr = Expr::LocalGet(obj_id);
        return;
    }

    // Issue #291 / #350: nested closures that captured `this` from the outer
    // method's frame need their own `Expr::This` → `LocalGet(obj_id)` rewrite
    // — after inlining the closure is hoisted into the call site's frame
    // (module init for top-level calls, where `this_stack` is empty), so the
    // codegen-side fallback can't recover a meaningful `this`. Substituting
    // here lets the closure run with the correct receiver.
    //
    // Also: explicitly add `obj_id` to the closure's captures list and clear
    // `captures_this` — the body now reads `LocalGet(obj_id)` rather than
    // `Expr::This`, and `compute_auto_captures` blends explicit + body-scanned
    // ids before excluding module globals, so adding to `captures` ensures the
    // receiver is forwarded through the closure's capture array regardless of
    // where the call site lands.
    //
    // `walk_expr_children_mut` deliberately does NOT recurse into Closure
    // bodies (per its module docs); we descend into the body explicitly
    // before falling through to the walker. Param.default exprs are visited
    // by the walker.
    if let Expr::Closure {
        body,
        captures,
        captures_this,
        ..
    } = expr
    {
        substitute_this_in_stmts(body, obj_id);
        *captures_this = false;
        if !captures.contains(&obj_id) {
            captures.push(obj_id);
        }
    }

    // Descend into every immediate sub-expression. The walker is exhaustive
    // on `Expr` — adding a new variant to `ir.rs` without updating
    // `walker.rs` is a compile error. This closes the bug class (issue #350)
    // where new HIR variants like `Expr::ArrayIsArray(inner)` containing
    // nested `PropertyGet → This` chains silently fell through the previous
    // ad-hoc match and left `Expr::This` references unsubstituted in inlined
    // method bodies — same shape as the v0.5.408 fix for the closure
    // collector (issue #318).
    walk_expr_children_mut(expr, &mut |child| substitute_this(child, obj_id));
}

/// Substitute Expr::This with a LocalGet reference in statements
fn substitute_this_in_stmts(stmts: &mut Vec<Stmt>, obj_id: LocalId) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::Let {
                init: Some(expr), ..
            } => {
                substitute_this(expr, obj_id);
            }
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                substitute_this(expr, obj_id);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                substitute_this(condition, obj_id);
                substitute_this_in_stmts(then_branch, obj_id);
                if let Some(else_b) = else_branch {
                    substitute_this_in_stmts(else_b, obj_id);
                }
            }
            Stmt::While { condition, body } => {
                substitute_this(condition, obj_id);
                substitute_this_in_stmts(body, obj_id);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    let mut init_vec = vec![*init_stmt.clone()];
                    substitute_this_in_stmts(&mut init_vec, obj_id);
                    if init_vec.len() == 1 {
                        **init_stmt = init_vec.remove(0);
                    }
                }
                if let Some(cond) = condition {
                    substitute_this(cond, obj_id);
                }
                if let Some(upd) = update {
                    substitute_this(upd, obj_id);
                }
                substitute_this_in_stmts(body, obj_id);
            }
            _ => {}
        }
    }
}

/// Substitute local variable references in statements
/// Collect all LocalIds defined by Let statements in a body (for remapping during inlining)
fn collect_body_local_ids(stmts: &[Stmt]) -> Vec<LocalId> {
    let mut ids = Vec::new();

    fn collect_from_stmt(stmt: &Stmt, ids: &mut Vec<LocalId>) {
        match stmt {
            Stmt::Let { id, .. } => {
                ids.push(*id);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                for s in then_branch {
                    collect_from_stmt(s, ids);
                }
                if let Some(else_b) = else_branch {
                    for s in else_b {
                        collect_from_stmt(s, ids);
                    }
                }
            }
            Stmt::While { body, .. } => {
                for s in body {
                    collect_from_stmt(s, ids);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_from_stmt(init_stmt, ids);
                }
                for s in body {
                    collect_from_stmt(s, ids);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    collect_from_stmt(s, ids);
                }
                if let Some(catch_clause) = catch {
                    // Also collect the catch parameter if present
                    if let Some((param_id, _)) = &catch_clause.param {
                        ids.push(*param_id);
                    }
                    for s in &catch_clause.body {
                        collect_from_stmt(s, ids);
                    }
                }
                if let Some(finally_stmts) = finally {
                    for s in finally_stmts {
                        collect_from_stmt(s, ids);
                    }
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    for s in &case.body {
                        collect_from_stmt(s, ids);
                    }
                }
            }
            _ => {}
        }
    }

    for stmt in stmts {
        collect_from_stmt(stmt, &mut ids);
    }
    ids
}

fn substitute_locals_in_stmts(
    stmts: &mut Vec<Stmt>,
    param_map: &HashMap<LocalId, Expr>,
    next_local_id: &mut LocalId,
) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::Let { id, init, .. } => {
                // Remap the Let's id if it's in the param_map
                if let Some(Expr::LocalGet(new_id)) = param_map.get(id) {
                    *id = *new_id;
                }
                if let Some(expr) = init {
                    substitute_locals(expr, param_map, next_local_id);
                }
            }
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                substitute_locals(expr, param_map, next_local_id);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                substitute_locals(condition, param_map, next_local_id);
                substitute_locals_in_stmts(then_branch, param_map, next_local_id);
                if let Some(else_b) = else_branch {
                    substitute_locals_in_stmts(else_b, param_map, next_local_id);
                }
            }
            Stmt::While { condition, body } => {
                substitute_locals(condition, param_map, next_local_id);
                substitute_locals_in_stmts(body, param_map, next_local_id);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    let mut init_vec = vec![*init_stmt.clone()];
                    substitute_locals_in_stmts(&mut init_vec, param_map, next_local_id);
                    if init_vec.len() == 1 {
                        **init_stmt = init_vec.remove(0);
                    }
                }
                if let Some(cond) = condition {
                    substitute_locals(cond, param_map, next_local_id);
                }
                if let Some(upd) = update {
                    substitute_locals(upd, param_map, next_local_id);
                }
                substitute_locals_in_stmts(body, param_map, next_local_id);
            }
            Stmt::PreallocateBoxes(ids) => {
                // Issue #569: remap each id in the prealloc list. Inlining
                // can rename body locals so the slot+box allocation must
                // refer to the new ids. If a callee with a PreallocateBoxes
                // gets inlined into a caller, its body's hoisted FnDecls
                // still need their boxes set up.
                for id in ids.iter_mut() {
                    if let Some(Expr::LocalGet(new_id)) = param_map.get(id) {
                        *id = *new_id;
                    }
                }
            }
            _ => {}
        }
    }
}

// ── Clamp-pattern detection (Issue #436 plan #1) ──────────────────────────
//
// These detectors mirror the ones in `perry-codegen/src/collectors.rs`
// (`detect_clamp3` / `detect_clamp_u8`). Duplicated rather than shared via
// `perry-hir` because perry-transform and perry-codegen are sibling crates;
// the patterns are tiny and purely syntactic, so drift is low-risk.
//
// Matched bodies are excluded from the inlinable-functions set so the
// `Expr::Call { callee: FuncRef(clamp_fn_id) }` shape survives at every
// call site. Codegen's `lower_expr_as_i32` and the f64-context arm in
// `lower_call.rs` then emit `@llvm.smin.i32` / `@llvm.smax.i32` inline,
// producing IR that LLVM's auto-vectorizer can lift.

/// `function clamp3(v, lo, hi) { if (v < lo) return lo; if (v > hi) return hi; return v; }`
fn is_clamp3(f: &Function) -> bool {
    if f.is_async || f.is_generator || f.params.len() != 3 || f.body.len() != 3 {
        return false;
    }
    let (v_id, lo_id, hi_id) = (f.params[0].id, f.params[1].id, f.params[2].id);
    let Stmt::If {
        condition:
            Expr::Compare {
                op: perry_hir::CompareOp::Lt,
                left,
                right,
            },
        then_branch,
        else_branch: None,
    } = &f.body[0]
    else {
        return false;
    };
    if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == v_id) {
        return false;
    }
    if !matches!(right.as_ref(), Expr::LocalGet(id) if *id == lo_id) {
        return false;
    }
    if then_branch.len() != 1
        || !matches!(&then_branch[0], Stmt::Return(Some(Expr::LocalGet(id))) if *id == lo_id)
    {
        return false;
    }
    let Stmt::If {
        condition:
            Expr::Compare {
                op: perry_hir::CompareOp::Gt,
                left,
                right,
            },
        then_branch,
        else_branch: None,
    } = &f.body[1]
    else {
        return false;
    };
    if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == v_id) {
        return false;
    }
    if !matches!(right.as_ref(), Expr::LocalGet(id) if *id == hi_id) {
        return false;
    }
    if then_branch.len() != 1
        || !matches!(&then_branch[0], Stmt::Return(Some(Expr::LocalGet(id))) if *id == hi_id)
    {
        return false;
    }
    matches!(&f.body[2], Stmt::Return(Some(Expr::LocalGet(id))) if *id == v_id)
}

/// `function clampU8(v) { if (v < 0) return 0; if (v > 255) return 255; return v|0; }`
fn is_clamp_u8(f: &Function) -> bool {
    if f.is_async || f.is_generator || f.params.len() != 1 || f.body.len() != 3 {
        return false;
    }
    let v_id = f.params[0].id;
    let Stmt::If {
        condition:
            Expr::Compare {
                op: perry_hir::CompareOp::Lt,
                left,
                right,
            },
        then_branch,
        else_branch: None,
    } = &f.body[0]
    else {
        return false;
    };
    if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == v_id) {
        return false;
    }
    if !matches!(right.as_ref(), Expr::Integer(0)) {
        return false;
    }
    if !matches!(
        then_branch.as_slice(),
        [Stmt::Return(Some(Expr::Integer(0)))]
    ) {
        return false;
    }
    let Stmt::If {
        condition:
            Expr::Compare {
                op: perry_hir::CompareOp::Gt,
                left,
                right,
            },
        then_branch,
        else_branch: None,
    } = &f.body[1]
    else {
        return false;
    };
    if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == v_id) {
        return false;
    }
    if !matches!(right.as_ref(), Expr::Integer(255)) {
        return false;
    }
    matches!(
        then_branch.as_slice(),
        [Stmt::Return(Some(Expr::Integer(255)))]
    )
}

// ── Math.imul polyfill detection ──────────────────────────────────────────

/// Detect whether a function is a Math.imul polyfill.
/// Matches the canonical pattern: 2 params, 4 half-word extraction Lets,
/// final Return with recombined multiply `| 0`.
fn detect_math_imul_polyfill(f: &Function) -> bool {
    if f.is_async || f.is_generator {
        return false;
    }
    if f.params.len() != 2 {
        return false;
    }
    if f.body.len() != 5 {
        return false;
    }

    let p0 = f.params[0].id;
    let p1 = f.params[1].id;

    // First 4 stmts must be immutable Lets with half-word extraction inits
    let mut hi_of = [false; 2]; // hi_of[0] = saw hi-half of p0, hi_of[1] = p1
    let mut lo_of = [false; 2];
    for stmt in &f.body[..4] {
        match stmt {
            Stmt::Let {
                mutable: false,
                init: Some(init),
                ..
            } => {
                if let Some((pid, is_hi)) = is_half_extract(init, p0, p1) {
                    let idx = if pid == p0 { 0 } else { 1 };
                    if is_hi {
                        hi_of[idx] = true;
                    } else {
                        lo_of[idx] = true;
                    }
                } else {
                    return false;
                }
            }
            _ => return false,
        }
    }
    if !(hi_of[0] && lo_of[0] && hi_of[1] && lo_of[1]) {
        return false;
    }

    // Last stmt: Return(Some(Binary { BitOr, ..., Integer(0) }))
    matches!(&f.body[4], Stmt::Return(Some(Expr::Binary { op: BinaryOp::BitOr, right, .. })) if matches!(right.as_ref(), Expr::Integer(0)))
}

/// Check if an expression extracts the hi or lo 16-bit half of a parameter.
/// Returns `Some((param_id, is_hi))` on match.
fn is_half_extract(e: &Expr, p0: LocalId, p1: LocalId) -> Option<(LocalId, bool)> {
    // Pattern: (param >>> 16) & 0xffff  OR  (param >> 16) & 0xffff  →  hi-half
    // Pattern: param & 0xffff  →  lo-half
    match e {
        Expr::Binary {
            op: BinaryOp::BitAnd,
            left,
            right,
        } => {
            if !matches!(right.as_ref(), Expr::Integer(0xffff)) {
                return None;
            }
            match left.as_ref() {
                Expr::Binary {
                    op: BinaryOp::UShr | BinaryOp::Shr,
                    left: inner,
                    right: shift_amt,
                } => {
                    if !matches!(shift_amt.as_ref(), Expr::Integer(16)) {
                        return None;
                    }
                    match inner.as_ref() {
                        Expr::LocalGet(id) if *id == p0 || *id == p1 => Some((*id, true)),
                        _ => None,
                    }
                }
                Expr::LocalGet(id) if *id == p0 || *id == p1 => Some((*id, false)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Rewrite `Call(FuncRef(imul_id), [a, b])` → `MathImul(a, b)` in statements.
fn rewrite_imul_calls_in_stmts(stmts: &mut [Stmt], imul_ids: &HashSet<FuncId>) {
    for s in stmts.iter_mut() {
        match s {
            Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => {
                rewrite_imul_calls_in_expr(e, imul_ids);
            }
            Stmt::Let { init: Some(e), .. } => {
                rewrite_imul_calls_in_expr(e, imul_ids);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                rewrite_imul_calls_in_expr(condition, imul_ids);
                rewrite_imul_calls_in_stmts(then_branch, imul_ids);
                if let Some(eb) = else_branch {
                    rewrite_imul_calls_in_stmts(eb, imul_ids);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                rewrite_imul_calls_in_expr(condition, imul_ids);
                rewrite_imul_calls_in_stmts(body, imul_ids);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    rewrite_imul_calls_in_stmts(std::slice::from_mut(init_stmt), imul_ids);
                }
                if let Some(c) = condition {
                    rewrite_imul_calls_in_expr(c, imul_ids);
                }
                if let Some(u) = update {
                    rewrite_imul_calls_in_expr(u, imul_ids);
                }
                rewrite_imul_calls_in_stmts(body, imul_ids);
            }
            _ => {}
        }
    }
}

fn rewrite_imul_calls_in_expr(e: &mut Expr, imul_ids: &HashSet<FuncId>) {
    // Check if this expr is a call to an imul polyfill
    let is_imul = matches!(e, Expr::Call { callee, args, .. }
        if args.len() == 2 && matches!(callee.as_ref(), Expr::FuncRef(fid) if imul_ids.contains(fid)));
    if is_imul {
        if let Expr::Call { args, .. } = std::mem::replace(e, Expr::Undefined) {
            let mut args = args;
            let b = args.pop().unwrap();
            let a = args.pop().unwrap();
            *e = Expr::MathImul(Box::new(a), Box::new(b));
        }
        // Recurse into the new MathImul operands
        if let Expr::MathImul(a, b) = e {
            rewrite_imul_calls_in_expr(a, imul_ids);
            rewrite_imul_calls_in_expr(b, imul_ids);
        }
        return;
    }

    // Recurse into sub-expressions
    match e {
        Expr::Binary { left, right, .. }
        | Expr::Logical { left, right, .. }
        | Expr::Compare { left, right, .. } => {
            rewrite_imul_calls_in_expr(left, imul_ids);
            rewrite_imul_calls_in_expr(right, imul_ids);
        }
        Expr::Unary { operand, .. } => rewrite_imul_calls_in_expr(operand, imul_ids),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            rewrite_imul_calls_in_expr(condition, imul_ids);
            rewrite_imul_calls_in_expr(then_expr, imul_ids);
            rewrite_imul_calls_in_expr(else_expr, imul_ids);
        }
        Expr::Call { callee, args, .. } => {
            rewrite_imul_calls_in_expr(callee, imul_ids);
            for arg in args {
                rewrite_imul_calls_in_expr(arg, imul_ids);
            }
        }
        Expr::LocalSet(_, val) => rewrite_imul_calls_in_expr(val, imul_ids),
        Expr::IndexGet { object, index } => {
            rewrite_imul_calls_in_expr(object, imul_ids);
            rewrite_imul_calls_in_expr(index, imul_ids);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            rewrite_imul_calls_in_expr(object, imul_ids);
            rewrite_imul_calls_in_expr(index, imul_ids);
            rewrite_imul_calls_in_expr(value, imul_ids);
        }
        Expr::Array(elems) => {
            for el in elems {
                rewrite_imul_calls_in_expr(el, imul_ids);
            }
        }
        Expr::PropertyGet { object, .. } => rewrite_imul_calls_in_expr(object, imul_ids),
        Expr::PropertySet { object, value, .. } => {
            rewrite_imul_calls_in_expr(object, imul_ids);
            rewrite_imul_calls_in_expr(value, imul_ids);
        }
        _ => {}
    }
}
