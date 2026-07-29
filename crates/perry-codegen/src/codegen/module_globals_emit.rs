//! Module-level global + static-class-field emission for `compile_module`.
//!
//! Extracted verbatim from the `compile_module` body (pure code move, no
//! behavior change). Pre-walks every function/method/closure body to find
//! which module-level `let`s escape the entry function (so they must be
//! globalized), emits the backing `@perry_global_*` globals + exported-var
//! getters, and registers/emits the `@perry_static_*` class-field globals.

use std::collections::HashMap;

use perry_hir::Module as HirModule;

use crate::module::LlModule;
use crate::types::DOUBLE;

// Collector and boxing-analysis walkers live in dedicated modules.
use crate::collectors::{collect_closures_in_stmts, collect_let_ids, collect_ref_ids_in_stmts};

// Name-mangling helpers + ImportedClass from the trunk (also via `super::*`).
use super::helpers::{sanitize, sanitize_member};
use super::ImportedClass;

/// Result bundle of the module-global + static-field emission pass.
pub(crate) struct ModuleGlobals {
    pub module_globals: HashMap<u32, String>,
    pub module_global_types: HashMap<u32, perry_hir::types::Type>,
    pub static_field_globals: HashMap<(String, String), String>,
}

/// Emit module-level globals (with exported-var getters) and static-class-field
/// globals. `compile_time_constants` supplies init values for known synthetic
/// consts (`__platform__`, `__plugins__`).
pub(crate) fn emit_module_globals(
    llmod: &mut LlModule,
    hir: &HirModule,
    imported_classes: &[ImportedClass],
    compile_time_constants: &HashMap<u32, f64>,
    module_prefix: &str,
) -> ModuleGlobals {
    // Module-level globals registry. Pre-walk:
    //   1. Collect every LocalId referenced from any function or method
    //      body (LocalGet / LocalSet / Update). Those that aren't a
    //      function/method's own param or Let must be module-level.
    //   2. Walk hir.init's top-level Lets and globalize ONLY the ones in
    //      that set. Lets that are only referenced from main itself stay
    //      as cheap stack alloca (preserves perf for the bench
    //      benchmarks that don't share state with helper functions).
    let mut referenced_from_fn: std::collections::HashSet<u32> = std::collections::HashSet::new();
    // Helper that handles "params + lets define a scope, refs minus
    // defines flow out". Used for every function/method/closure body.
    let scan_body = |params: &[perry_hir::Param],
                     body: &[perry_hir::Stmt],
                     out: &mut std::collections::HashSet<u32>| {
        let mut local_defs: std::collections::HashSet<u32> = params.iter().map(|p| p.id).collect();
        collect_let_ids(body, &mut local_defs);
        let mut refs: std::collections::HashSet<u32> = std::collections::HashSet::new();
        collect_ref_ids_in_stmts(body, &mut refs);
        for r in refs {
            if !local_defs.contains(&r) {
                out.insert(r);
            }
        }
    };
    for f in &hir.functions {
        scan_body(&f.params, &f.body, &mut referenced_from_fn);
    }
    for c in &hir.classes {
        for m in &c.methods {
            scan_body(&m.params, &m.body, &mut referenced_from_fn);
        }
        if let Some(ctor) = &c.constructor {
            scan_body(&ctor.params, &ctor.body, &mut referenced_from_fn);
        }
        // Issue #2310 — static methods, getters/setters, and
        // (static) field initializers were missing here, so a
        // module-level `let n = 0; class C { static bump() { return
        // n++; } }` left `n` un-globalized — codegen routed `n++` to
        // a local alloca whose value was never observed by anything
        // outside the static method, and reads via
        // `_cjs.C.bump()` came back 0 every call. Including these
        // bodies in the reference scan lets the `referenced_from_fn`
        // → `module_globals` promotion below catch the same pattern
        // as instance methods.
        for sm in &c.static_methods {
            scan_body(&sm.params, &sm.body, &mut referenced_from_fn);
        }
        for member in &c.computed_members {
            scan_body(
                &member.function.params,
                &member.function.body,
                &mut referenced_from_fn,
            );
        }
        for (_, getter_fn) in &c.getters {
            scan_body(&getter_fn.params, &getter_fn.body, &mut referenced_from_fn);
        }
        for (_, setter_fn) in &c.setters {
            scan_body(&setter_fn.params, &setter_fn.body, &mut referenced_from_fn);
        }
        // Field initializers are evaluated inside the constructor —
        // most carry module-global refs only when they're closures
        // (already walked by the closure pass below). Wrap each init
        // expression as a synthetic `Stmt::Expr` so direct refs (like
        // `static seed = RANDOM_POOL_SIZE`) also surface here.
        for field in &c.fields {
            if let Some(init) = &field.init {
                scan_body(
                    &[],
                    &[perry_hir::Stmt::Expr(init.clone())],
                    &mut referenced_from_fn,
                );
            }
        }
        for field in &c.static_fields {
            if let Some(init) = &field.init {
                scan_body(
                    &[],
                    &[perry_hir::Stmt::Expr(init.clone())],
                    &mut referenced_from_fn,
                );
            }
        }
    }
    // Also walk every closure body. A self-referencing recursive
    // closure (`let f = (n) => f(n-1)`) needs `f` to be globalized
    // so the closure body can see the live storage instead of a
    // stale snapshot. Without this, the closure auto-capture sees
    // `f` is not yet declared and bails with "local not in scope".
    {
        let mut closures: Vec<(perry_hir::types::FuncId, perry_hir::Expr)> = Vec::new();
        let mut seen: std::collections::HashSet<perry_hir::types::FuncId> =
            std::collections::HashSet::new();
        for f in &hir.functions {
            collect_closures_in_stmts(&f.body, &mut seen, &mut closures);
        }
        for c in &hir.classes {
            for m in &c.methods {
                collect_closures_in_stmts(&m.body, &mut seen, &mut closures);
            }
            for (_, getter_fn) in &c.getters {
                collect_closures_in_stmts(&getter_fn.body, &mut seen, &mut closures);
            }
            for (_, setter_fn) in &c.setters {
                collect_closures_in_stmts(&setter_fn.body, &mut seen, &mut closures);
            }
            for sm in &c.static_methods {
                collect_closures_in_stmts(&sm.body, &mut seen, &mut closures);
            }
            for member in &c.computed_members {
                collect_closures_in_stmts(&member.function.body, &mut seen, &mut closures);
            }
            if let Some(ctor) = &c.constructor {
                collect_closures_in_stmts(&ctor.body, &mut seen, &mut closures);
            }
            // Class field initializers (`private foo = (x) => this.bar(x)`) are
            // hoisted into the constructor at codegen time via
            // `apply_field_initializers_recursive`, so any closure literal inside
            // an `init` expression gets a `js_closure_alloc(@perry_closure_*)`
            // emission. We must walk the inits too, otherwise the body never
            // gets compiled and clang errors with "use of undefined value" (#261).
            for field in &c.fields {
                if let Some(init) = &field.init {
                    collect_closures_in_stmts(
                        &[perry_hir::Stmt::Expr(init.clone())],
                        &mut seen,
                        &mut closures,
                    );
                }
            }
            // #338: same gap as the main compile loop — static field inits
            // (`static make = (x) => ...`) need walking so the global-
            // detection pre-walk sees their captures and globalises any
            // module-level lets the closure body references.
            for field in &c.static_fields {
                if let Some(init) = &field.init {
                    collect_closures_in_stmts(
                        &[perry_hir::Stmt::Expr(init.clone())],
                        &mut seen,
                        &mut closures,
                    );
                }
            }
        }
        collect_closures_in_stmts(&hir.init, &mut seen, &mut closures);
        for (_, closure_expr) in &closures {
            if let perry_hir::Expr::Closure { params, body, .. } = closure_expr {
                scan_body(params, body, &mut referenced_from_fn);
            }
        }
    }

    let mut module_globals: HashMap<u32, String> = HashMap::new();
    // Module global types: propagated to every FnCtx so functions that
    // access module globals (via LocalGet/LocalSet) see the correct
    // declared type. Without this, `editorInstance` (Named("Editor"))
    // in render.ts has its type only in the entry function's FnCtx,
    // so method calls in other functions fall through to the generic
    // dispatch instead of the class method registry.
    let mut module_global_types: HashMap<u32, perry_hir::types::Type> = HashMap::new();
    // Collect exported variable names so we can create external
    // globals + getter functions for cross-module access.
    let exported_var_names: std::collections::HashSet<String> =
        hir.exported_objects.iter().cloned().collect();
    // #6649: module-level array-destructuring declarations (`var [Prime, Size]
    // = [BigInt(...), BigInt(...)]` — TypeBox's FNV-1a table in the pi bundle)
    // lower their leaf `Stmt::Let`s inside the iterator-protocol `Stmt::Try`
    // scaffolding (IteratorClose on abrupt completion), so the previous
    // top-level-only scan never saw them. The leaves then stayed
    // un-globalized and every function/method/closure reference compiled to
    // the not-in-scope fallback (`undefined`): TypeBox's `Accumulator * Prime`
    // saw `ToNumeric(undefined) = NaN` and threw a spurious "Cannot mix BigInt
    // and other types" during pi-native init. Walk through Try scaffolding
    // (body/catch/finally, transitively for nested patterns) when collecting
    // candidate lets — a module-init try body runs at most once, so its
    // bindings are single-instance and safe to promote. Loop and if bodies
    // intentionally stay out of the walk: their `let`s are genuinely
    // block-scoped (fresh binding per iteration) and remain handled by the
    // boxed-capture machinery.
    fn collect_init_lets<'a>(stmts: &'a [perry_hir::Stmt], out: &mut Vec<&'a perry_hir::Stmt>) {
        for s in stmts {
            match s {
                perry_hir::Stmt::Let { .. } => out.push(s),
                perry_hir::Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    collect_init_lets(body, out);
                    if let Some(c) = catch {
                        collect_init_lets(&c.body, out);
                    }
                    if let Some(f) = finally {
                        collect_init_lets(f, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut init_lets: Vec<&perry_hir::Stmt> = Vec::new();
    collect_init_lets(&hir.init, &mut init_lets);
    for s in init_lets {
        if let perry_hir::Stmt::Let { id, name, ty, .. } = s {
            // Always record the declared type for module-level lets
            // so all functions see it (not just the entry function).
            if !matches!(ty, perry_hir::types::Type::Any) {
                module_global_types.insert(*id, ty.clone());
            }
            if referenced_from_fn.contains(id) || exported_var_names.contains(name) {
                // A `var` redeclared at module scope (`var x = …; … var x = …;`)
                // lowers to multiple `Stmt::Let` sharing the SAME id. The backing
                // global (and any exported getter) is keyed by that id, so emit it
                // exactly once — a second `add_global` for the same symbol is an
                // LLVM "redefinition of global" hard error. Captured + redeclared
                // module vars are the trigger (e.g. test262 capability tests).
                if module_globals.contains_key(id) {
                    continue;
                }
                // Use external linkage for exported vars so other
                // modules can reference them. Internal for the rest.
                let is_exported = exported_var_names.contains(name);
                let global_name = format!("perry_global_{}__{}", module_prefix, id);
                // Use the compile-time constant value if one was registered
                // (e.g., __platform__, __plugins__). Otherwise default to 0.0.
                let init_value = if let Some(cv) = compile_time_constants.get(id) {
                    format!("{:.1}", cv)
                } else {
                    crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                // Use default (external) linkage for ALL module globals.
                // `internal` linkage lets clang -O3 assume the global is
                // never written by optnone functions (setjmp/try-catch),
                // causing it to constant-fold reads to 0.0. With external
                // linkage, the optimizer can't make cross-TU assumptions.
                // The module-unique name (perry_global_<prefix>__N)
                // prevents symbol collisions across modules.
                llmod.add_global(&global_name, DOUBLE, &init_value);
                module_globals.insert(*id, global_name.clone());

                // For exported variables, also emit a trivial getter
                // function `perry_fn_<prefix>__<name>` that returns
                // the global. The ExternFuncRef wrapper in importing
                // modules calls this symbol — without it, exported
                // constants (like `export const Key = { ... }`) cause
                // linker errors because the wrapper tries to call a
                // function that doesn't exist.
                // Skip the getter for names that are also functions — the
                // compiled function body will provide the correct symbol.
                // Without this, `export function isSetupComplete()` gets
                // a trivial getter that wraps a broken _i64 stub (returns 0)
                // instead of the real function that reads the module global.
                let is_also_function = hir
                    .functions
                    .iter()
                    .any(|f| f.is_exported && f.name == *name);
                // Also skip the value-getter when this name is already an
                // exported function alias (e.g. `export const async = _async`
                // or `export { _void as void }`). For those the #460 forwarding
                // wrapper below emits a `perry_fn_<modprefix>__<name>`
                // definition that actually calls the underlying function;
                // emitting a getter here on top would be a redef and is
                // semantically wrong (it'd return the closure value instead
                // of invoking it).
                let is_function_alias = hir.exported_functions.iter().any(|(exp, _)| exp == name);
                if is_exported && !is_also_function && !is_function_alias {
                    let fn_name = format!("perry_fn_{}__{}", module_prefix, sanitize(name),);
                    let getter = llmod.define_function(&fn_name, DOUBLE, vec![]);
                    let _ = getter.create_block("entry");
                    let blk = getter.block_mut(0).unwrap();
                    let val = blk.load(DOUBLE, &format!("@{}", global_name));
                    blk.ret(DOUBLE, &val);

                    // #460: also emit a duplicate getter under any renamed
                    // export targeting this local. `export { _await as await }`
                    // means consumers compute the callee symbol from the
                    // exported name `await` — without an alias getter the
                    // link fails on `_perry_fn_<mod>__<keyword>`. The wrapper
                    // returns the same global value the local-name getter
                    // returns; callers that invoke it as a function get the
                    // closure handle (matching status quo for non-renamed
                    // `export const f = aFunctionRef` exports).
                    for export in &hir.exports {
                        if let perry_hir::Export::Named { local, exported } = export {
                            if local == name && exported != name {
                                let alias_fn =
                                    format!("perry_fn_{}__{}", module_prefix, sanitize(exported));
                                if alias_fn == fn_name {
                                    continue;
                                }
                                let g = llmod.define_function(&alias_fn, DOUBLE, vec![]);
                                let _ = g.create_block("entry");
                                let b = g.block_mut(0).unwrap();
                                let v = b.load(DOUBLE, &format!("@{}", global_name));
                                b.ret(DOUBLE, &v);
                            }
                        }
                    }
                }
            }
        }
    }

    // Phase E: register and emit static class fields as module globals.
    // Each `static foo: T = init` becomes `@perry_static_<modprefix>__
    // <class>__<field>` initialized to 0.0. The init expression runs
    // in compile_module_entry's main/init function before user code.
    let mut static_field_globals: HashMap<(String, String), String> = HashMap::new();
    // Track which `@perry_static_*` globals we've already emitted (defining or
    // external) so a repeated symbol — a duplicate static field name within one
    // class (#5345), or the same imported class pulled in twice — never emits a
    // second LLVM global, which clang rejects as a redefinition.
    let mut external_globals_emitted: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for c in &hir.classes {
        for sf in &c.static_fields {
            // Computed-key static fields (`static [Symbol.for(...)] = init`)
            // are stored in a runtime side table by
            // `init_static_fields_late`; they don't get a string-named
            // global. Refs #420, #894.
            if sf.key_expr.is_some() {
                continue;
            }
            let name = format!(
                "perry_static_{}__{}__{}",
                module_prefix,
                sanitize_member(&c.name),
                sanitize_member(&sf.name),
            );
            // External linkage so importing modules can reference the same
            // global. Static class fields are spec-level shared state across
            // the whole program (same `Symbol.X` value seen everywhere); they
            // must be a single defining global, not per-module copies.
            // Refs #420: drizzle's `Sub extends Base` reads `[Base.Symbol.X]`
            // when Sub is in a different file from Base; without external
            // linkage, the importing module's `StaticFieldGet { Base, Symbol }`
            // had no symbol to resolve and silently produced 0.0.
            //
            // #5345: a class may declare the SAME static field name twice
            // (`static f = 'a'; static f = this.f + 'b';`) — both initializers
            // run in declaration order against one shared slot (last write
            // wins). They mangle to the same global symbol, so emit the
            // defining global only once; clang rejects a redefined `@…__f`.
            // The init loop still walks every `c.static_fields` entry, so both
            // assignments execute against this single slot.
            if external_globals_emitted.insert(name.clone()) {
                llmod.add_global(&name, DOUBLE, "0.0");
            }
            static_field_globals.insert((c.name.clone(), sf.name.clone()), name);
        }
    }
    // Register foreign static-field globals from imported classes. The source
    // module emits the defining external global (above); the consumer just
    // declares a reference and adds it to its own `static_field_globals` map
    // so `Expr::StaticFieldGet/Set` lowering finds it.
    // (external_globals_emitted is declared above, shared with the local-class
    // loop, to avoid double-declarations.)
    for ic in imported_classes {
        let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);
        // Skip imported-class entries whose source matches this module's
        // prefix — the local-class loop above already emitted the defining
        // global. Re-declaring as external would produce a duplicate-symbol
        // error in the LLVM IR (clang rejects `@x = global` next to `@x =
        // external global`). Same-named local classes also win.
        if ic.source_prefix == module_prefix {
            // Still register in the static_field_globals map so HIR lookups
            // by the imported alias resolve to the local definition.
            for sf_name in &ic.static_field_names {
                let key = (effective_name.to_string(), sf_name.clone());
                static_field_globals.entry(key).or_insert_with(|| {
                    let global_name = format!(
                        "perry_static_{}__{}__{}",
                        module_prefix,
                        sanitize_member(&ic.name),
                        sanitize_member(sf_name),
                    );
                    global_name
                });
            }
            continue;
        }
        if hir.classes.iter().any(|c| c.name == ic.name) {
            continue;
        }
        for sf_name in &ic.static_field_names {
            let global_name = format!(
                "perry_static_{}__{}__{}",
                ic.source_prefix,
                sanitize_member(&ic.name),
                sanitize_member(sf_name),
            );
            // Declare external (not define) — the source module owns the
            // defining global. Skip if already declared (multiple imports of
            // the same class).
            if external_globals_emitted.insert(global_name.clone()) {
                llmod.add_external_global(&global_name, DOUBLE);
            }
            // Register under both the alias (if any) and the source name so
            // either resolves.
            static_field_globals.insert(
                (effective_name.to_string(), sf_name.clone()),
                global_name.clone(),
            );
            if effective_name != ic.name {
                static_field_globals.insert((ic.name.clone(), sf_name.clone()), global_name);
            }
        }
    }

    ModuleGlobals {
        module_globals,
        module_global_types,
        static_field_globals,
    }
}
