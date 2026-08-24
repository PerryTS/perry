//! Static class-field and static-block initialization.
//!
//! Split out of `helpers.rs` (2000-line-per-file cap). Pure relocation --
//! `init_static_fields_early` / `init_static_fields_late` and the
//! `collect_inline_invoked_static_blocks` helper they share.

use super::helpers::*;
use super::*;

/// Early static-field setup: registrations that don't read any
/// module-level binding's value (Error-extending classes, well-known
/// symbol method hooks). Safe to emit before `stmt::lower_stmts` —
/// values referenced are either compile-time constants (class ids,
/// function pointers) or computed entirely from `hir` metadata.
///
/// The split (early vs. late) was introduced for issue #894 (effect's
/// `make()` factory's `static [TypeId] = variance` — both the key and
/// the init reference module-level lets that haven't been initialized
/// at the point the old combined `init_static_fields` ran).
pub(super) fn init_static_fields_early(
    ctx: &mut crate::expr::FnCtx<'_>,
    hir: &HirModule,
) -> Result<()> {
    // Phase C.3: register user classes that extend the built-in Error
    // (or any of its subclasses) with the runtime, so `instanceof Error`
    // walks the chain and returns true. Without this, `new HttpError(...)
    // instanceof Error` returns false because the runtime's
    // `EXTENDS_ERROR_REGISTRY` is empty for user classes.
    for c in &hir.classes {
        // Walk this class's extends_name chain; if any ancestor is a
        // built-in error subclass, register this class's id.
        let mut cur: Option<String> = c.extends_name.clone();
        let mut extends_error = false;
        let mut extends_data_view = false;
        let mut extends_typed_array = false;
        let mut depth = 0usize;
        while let Some(name) = cur {
            if matches!(
                name.as_str(),
                "Error"
                    | "TypeError"
                    | "RangeError"
                    | "ReferenceError"
                    | "SyntaxError"
                    | "URIError"
                    | "EvalError"
                    | "AggregateError"
            ) {
                extends_error = true;
                break;
            }
            if name == "DataView" {
                extends_data_view = true;
                break;
            }
            if crate::type_analysis::is_typed_array_class(&name) {
                extends_typed_array = true;
                break;
            }
            // Walk user-defined ancestor chain.
            if let Some(parent) = ctx.classes.get(&name) {
                cur = parent.extends_name.clone();
                depth += 1;
                if depth > 32 {
                    break;
                }
            } else {
                cur = None;
            }
        }
        if extends_error {
            if let Some(&cid) = ctx.class_ids.get(&c.name) {
                let cid_str = cid.to_string();
                ctx.block().call_void(
                    "js_register_class_extends_error",
                    &[(crate::types::I32, &cid_str)],
                );
            }
        }
        if extends_data_view {
            if let Some(&cid) = ctx.class_ids.get(&c.name) {
                let cid_str = cid.to_string();
                ctx.block().call_void(
                    "js_register_class_extends_data_view",
                    &[(crate::types::I32, &cid_str)],
                );
            }
        }
        if extends_typed_array {
            if let Some(&cid) = ctx.class_ids.get(&c.name) {
                ctx.block().call_void(
                    "js_register_class_extends_typed_array",
                    &[(crate::types::I32, &cid.to_string())],
                );
            }
        }
    }
    // Well-known symbol class hooks: HIR lifts `static [Symbol.hasInstance]`
    // and `get [Symbol.toStringTag]` to top-level functions with the
    // prefixes `__perry_wk_hasinstance_<class>` / `__perry_wk_tostringtag_<class>`.
    // Scan `hir.functions`, compute the LLVM symbol via `scoped_fn_name`,
    // and emit `js_register_class_<hook>(class_id, ptrtoint(@func, i64))`
    // at module init so the runtime's `js_instanceof` / `js_object_to_string`
    // can dispatch through them.
    let module_prefix = ctx.strings.module_prefix().to_string();
    for f in &hir.functions {
        let (registrar, class_name): (&str, &str) =
            if let Some(rest) = f.name.strip_prefix("__perry_wk_hasinstance_") {
                ("js_register_class_has_instance", rest)
            } else if let Some(rest) = f.name.strip_prefix("__perry_wk_tostringtag_") {
                ("js_register_class_to_string_tag", rest)
            } else {
                continue;
            };
        let Some(&cid) = ctx.class_ids.get(class_name) else {
            continue;
        };
        let cid_str = cid.to_string();
        let llvm_sym = format!("perry_fn_{}__{}", module_prefix, sanitize(&f.name));
        let func_ref = format!("@{}", llvm_sym);
        let blk = ctx.block();
        let func_ptr_i64 = blk.ptrtoint(&func_ref, I64);
        blk.call_void(
            registrar,
            &[(crate::types::I32, &cid_str), (I64, &func_ptr_i64)],
        );
    }
    // Uninitialized, non-computed static fields (`static foo;`, `static "g";`,
    // `static 0;`) are own data properties of the constructor with value
    // `undefined` per ClassDefinitionEvaluation. Their value is a compile-time
    // constant (`undefined`) with no dependency on user lets, and a class name
    // is in TDZ before its declaration, so registering them here — before user
    // code — is observably identical to registering at the class-decl position
    // and strictly earlier than the `init_static_fields_late` fallback that
    // previously handled them (which ran AFTER user statements, so
    // `Object.keys(C)` / `getOwnPropertyDescriptor(C, "foo")` immediately after
    // the declaration saw nothing). test262 class/elements static-as-valid-
    // static-field & friends. Initialized and computed-key fields are emitted
    // inline at their source position elsewhere and are skipped here.
    for c in &hir.classes {
        // Next.js wall 54: a nested class (declared inside a function) has its
        // static-field initializers run when the enclosing function evaluates
        // the class, NOT at module init — skip them here.
        if c.is_nested {
            continue;
        }
        let Some(&class_id) = ctx.class_ids.get(&c.name) else {
            continue;
        };
        if class_id == 0 {
            continue;
        }
        for sf in &c.static_fields {
            if sf.key_expr.is_some() || sf.init.is_some() || sf.name.starts_with('#') {
                continue;
            }
            let idx = ctx.strings.intern(&sf.name);
            let entry = ctx.strings.entry(idx);
            let bytes_ref = format!("@{}", entry.bytes_global);
            let len_str = entry.byte_len.to_string();
            let cid_str = class_id.to_string();
            let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            ctx.block().call_void(
                "js_class_register_static_field",
                &[
                    (crate::types::I32, &cid_str),
                    (crate::types::PTR, &bytes_ref),
                    (crate::types::I64, &len_str),
                    (DOUBLE, &undef),
                ],
            );
        }
    }
    Ok(())
}

/// Late static-field setup: per-class static-field initializer evaluation,
/// computed-Symbol-key registration, and static-block invocation. Must
/// run AFTER `stmt::lower_stmts` so module-level lets referenced by
/// these initializers (e.g. `static [TypeId] = variance` where both
/// `TypeId` and `variance` are top-level `const`s) read their populated
/// global slots rather than the zero default.
///
/// Issue #894: effect's `function make(ast) { return class { static
/// [TypeId] = variance } }` factory pattern hit this; the `TypeId`
/// symbol and `variance` value were both top-level module lets, and
/// the pre-#894 combined `init_static_fields` ran before user init,
/// so `js_class_register_static_symbol(class_id, 0.0, 0.0)` registered
/// nothing reachable. `isSchema(C)` then returned false on a class
/// returned from `make`, dual()'s predicate failed, and the failing
/// `.annotations({...})` chain eventually fed `undefined` to a `make`
/// call that read `ast._tag` → `TypeError: Cannot read properties of
/// undefined (reading '_tag')` during Schema.ts module init.
pub(super) fn init_static_fields_late(
    ctx: &mut crate::expr::FnCtx<'_>,
    hir: &HirModule,
) -> Result<()> {
    // Issue #685: nested classes (declared as expressions inside a
    // factory function body, e.g. `return class X extends Y { static
    // params = params.slice() }` in effect's `TemplateLiteralParser`)
    // are hoisted into `module.classes` by HIR lowering, but their
    // static-field initializers may reference parameters of the
    // enclosing function — those LocalIds aren't in the module-init
    // scope. The fallback at `expr.rs::LocalGet` returns `0.0`, so the
    // hoisted init becomes `(0.0).slice()` and throws
    // `TypeError: (number).slice is not a function` deep in
    // `<module>__init`, before any user code runs.
    //
    // Skip such inits at module level — the static field's storage
    // remains the zero default, which is wrong but harmless (the class
    // is built fresh on each factory invocation and the static slot
    // would need re-emitting per-invocation to be correct). The full
    // fix is to emit the init at the class-expression site inside the
    // factory body; tracking the eager-eval-of-inner-class-statics
    // separately.
    let mut module_local_scope: std::collections::HashSet<u32> =
        ctx.module_globals.keys().copied().collect();
    // Top-level `let` / `const` bindings may not appear in
    // `module_globals` (the global table only includes vars referenced
    // from inner functions or exported). For the purpose of "is this
    // LocalId in the module's own scope," count every top-level
    // `Stmt::Let` id too — otherwise a valid
    // `static foo = topLevelConst` would be wrongly skipped.
    for s in &hir.init {
        if let perry_hir::Stmt::Let { id, .. } = s {
            module_local_scope.insert(*id);
        }
    }
    let init_references_out_of_scope_local = |init_expr: &perry_hir::Expr| -> bool {
        let mut refs: std::collections::HashSet<u32> = std::collections::HashSet::new();
        crate::collectors::collect_ref_ids_in_expr(init_expr, &mut refs);
        refs.iter().any(|id| !module_local_scope.contains(id))
    };
    for c in &hir.classes {
        // Next.js wall 54: a nested class's static-field initializers must run
        // when the enclosing function evaluates the class, not at module init.
        // Running a side-effectful one eagerly (e.g. `static #a = new Self()`)
        // both mistimes it and can crash before user code.
        if c.is_nested {
            continue;
        }
        for sf in &c.static_fields {
            // Computed-key static fields go through the class-static-symbol
            // side table. Refs #420 — drizzle's `static [entityKind] =
            // "Table"` is consulted by `Object.prototype.hasOwnProperty.call(
            // type, entityKind)` in drizzle's `is(value, type)`.
            if let (Some(key_expr), Some(init_expr)) = (sf.key_expr.as_ref(), sf.init.as_ref()) {
                if init_references_out_of_scope_local(init_expr)
                    || init_references_out_of_scope_local(key_expr)
                {
                    continue;
                }
                let Some(&class_id) = ctx.class_ids.get(&c.name) else {
                    continue;
                };
                let key_v = crate::expr::lower_expr(ctx, key_expr)?;
                let val_v = crate::expr::lower_expr(ctx, init_expr)?;
                let cid_str = class_id.to_string();
                ctx.block().call_void(
                    "js_class_register_static_symbol",
                    &[
                        (crate::types::I32, &cid_str),
                        (DOUBLE, &key_v),
                        (DOUBLE, &val_v),
                    ],
                );
                continue;
            }
            let key = (c.name.clone(), sf.name.clone());
            // Register the field in the runtime CLASS_DYNAMIC_PROPS side
            // table (mirroring the StaticFieldSet lowering) so dynamic
            // class-ref reads and `getOwnPropertyDescriptor(C, name)` see an
            // own data property. Uninitialized fields (`static h;`) register
            // `undefined` — per spec they are still own properties.
            let emit_static_field_registration = |ctx: &mut crate::expr::FnCtx<'_>, value: &str| {
                if let Some(&class_id) = ctx.class_ids.get(&c.name) {
                    if class_id != 0 {
                        let idx = ctx.strings.intern(&sf.name);
                        let entry = ctx.strings.entry(idx);
                        let bytes_ref = format!("@{}", entry.bytes_global);
                        let len_str = entry.byte_len.to_string();
                        let cid_str = class_id.to_string();
                        ctx.block().call_void(
                            "js_class_register_static_field",
                            &[
                                (crate::types::I32, &cid_str),
                                (crate::types::PTR, &bytes_ref),
                                (crate::types::I64, &len_str),
                                (DOUBLE, value),
                            ],
                        );
                    }
                }
            };
            let Some(global_name) = ctx.static_field_globals.get(&key).cloned() else {
                continue;
            };
            if let Some(init_expr) = &sf.init {
                if init_references_out_of_scope_local(init_expr) {
                    continue;
                }
                // Skip fields whose initializer the HIR already emitted as an
                // inline `StaticFieldSet` at the class's source position (the
                // spec evaluation point). Re-running it here would (a) fire
                // initializer side effects twice and (b) clobber any user
                // reassignment made between the class decl and end of module
                // init. Mirrors the static-block dedup below. The inline
                // lowering also registers the field in CLASS_DYNAMIC_PROPS.
                let inline_initialized = hir.init.iter().any(|s| {
                    matches!(
                        s,
                        perry_hir::Stmt::Expr(perry_hir::Expr::StaticFieldSet {
                            class_name,
                            field_name,
                            ..
                        }) if *class_name == c.name && *field_name == sf.name
                    )
                });
                if inline_initialized {
                    continue;
                }
                // `this` in a static field initializer is the class
                // constructor (`static g = this.f + '262'`). Seed the same
                // class-ref NaN-box a static method binds (see
                // `compile_static_method`) for the init's duration.
                let seeded_this = ctx.class_ids.get(&c.name).copied().map(|cid| {
                    let bits = crate::nanbox::INT32_TAG | (cid as u64 & 0xFFFF_FFFF);
                    let class_ref_lit = crate::nanbox::double_literal(f64::from_bits(bits));
                    let this_slot = ctx.func.alloca_entry(DOUBLE);
                    ctx.block().store(DOUBLE, &class_ref_lit, &this_slot);
                    ctx.this_stack.push(this_slot);
                });
                let v = crate::expr::lower_expr(ctx, init_expr);
                if seeded_this.is_some() {
                    ctx.this_stack.pop();
                }
                let v = v?;
                let g_ref = format!("@{}", global_name);
                crate::expr::emit_root_nanbox_store_on_block(ctx.block(), &v, &g_ref);
                emit_static_field_registration(ctx, &v);
            }
            // Uninitialized non-computed static fields are now registered in
            // `init_static_fields_early` (before user code) with value
            // `undefined`. Re-registering here — after user statements — would
            // clobber any `C.foo = …` the program performed between the class
            // declaration and module-init end, so the no-init `else` branch was
            // intentionally removed.
        }
    }
    // Static blocks — emitted as synthetic static methods with the
    // name prefix `__perry_static_init_`. HIR lowering injects an inline
    // `StaticMethodCall` for each one at the class-decl source position
    // (right after that class's static-field-init stmts), so blocks
    // normally run from `hir.init`. This loop is a fallback for any
    // class whose static_methods include a block not yet hooked via
    // init (e.g. class expressions that bypass the stmt-decl path);
    // calling it here keeps the legacy behavior of "always run, just
    // late" for those. (#2278)
    // #5989: blocks already invoked inline at their class's evaluation point —
    // module top level (a top-level class decl), a function body, OR a nested
    // closure (a function-nested class decl, whose block call `lower_decl::
    // body_stmt` emits into its factory/closure body). The module-init fallback
    // below must NOT ALSO run those: a nested class's block would fire at module
    // init, before its factory binds the block's captured factory-locals, so a
    // block reading a lazy import (`class m { static { this.contextType =
    // g.AppRouterContext } }`, `g = a.i(N)`) threw in `<module>__init` (Next.js
    // /plain App Router chunk). Class EXPRESSIONS with no inline invocation are
    // absent from this set and still run at module init (the fallback's purpose).
    let inline_invoked = collect_inline_invoked_static_blocks(hir);
    for c in &hir.classes {
        for sm in &c.static_methods {
            if !sm.name.starts_with("__perry_static_init_") {
                continue;
            }
            if inline_invoked.contains(&(c.name.clone(), sm.name.clone())) {
                continue;
            }
            let key = (
                c.name.clone(),
                crate::codegen::static_method_registry_key(&sm.name),
            );
            if let Some(llvm_name) = ctx.methods.get(&key).cloned() {
                ctx.block().call(DOUBLE, &llvm_name, &[]);
            }
        }
    }
    Ok(())
}

/// #5989: collect every `(class, method)` invoked via a `StaticMethodCall`
/// ANYWHERE in the module — module init, top-level function bodies, and
/// (crucially) recursively inside nested closures. `init_calls_static_block`
/// only walks statement-level control flow, so a block call buried in a
/// factory/closure body (a function-nested class decl's inline invocation,
/// emitted by `lower_decl::body_stmt`) is invisible to it.
///
/// `init_static_fields_late` uses this to skip a static block that already has
/// an inline invocation at the point its class is evaluated. Without it, such a
/// block ALSO ran at module init — before its factory bound the block's captured
/// factory-locals — so a nested class whose block reads a lazy import
/// (`class m { static { this.contextType = g.AppRouterContext } }`, `g = a.i(N)`)
/// threw in `<module>__init` (Next.js /plain App Router chunk). Class EXPRESSIONS
/// with no inline invocation are NOT collected and still run at module init.
fn collect_inline_invoked_static_blocks(
    hir: &HirModule,
) -> std::collections::HashSet<(String, String)> {
    use perry_hir::{Expr, Stmt};
    let mut out: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();

    fn walk_expr(e: &Expr, out: &mut std::collections::HashSet<(String, String)>) {
        if let Expr::StaticMethodCall {
            class_name,
            method_name,
            ..
        } = e
        {
            out.insert((class_name.clone(), method_name.clone()));
        }
        // `ClassExprFresh` invokes its static blocks directly from the
        // per-evaluation source-order plan. Treat those calls as inline too;
        // otherwise the module-init fallback below invokes every block once
        // more with no fresh class object armed as `this`.
        if let Expr::ClassExprFresh {
            template,
            static_init_order,
            ..
        } = e
        {
            for step in static_init_order {
                if let perry_hir::ClassFreshStaticInit::Block(index) = step {
                    out.insert((template.clone(), format!("__perry_static_init_{index}")));
                }
            }
        }
        if let Expr::Closure { body, .. } = e {
            for s in body {
                walk_stmt(s, out);
            }
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| walk_expr(c, out));
    }

    fn walk_stmt(s: &Stmt, out: &mut std::collections::HashSet<(String, String)>) {
        match s {
            Stmt::Let { init: Some(e), .. } => walk_expr(e, out),
            Stmt::Expr(e) | Stmt::Throw(e) => walk_expr(e, out),
            Stmt::Return(Some(e)) => walk_expr(e, out),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                walk_expr(condition, out);
                then_branch.iter().for_each(|s| walk_stmt(s, out));
                if let Some(eb) = else_branch {
                    eb.iter().for_each(|s| walk_stmt(s, out));
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                walk_expr(condition, out);
                body.iter().for_each(|s| walk_stmt(s, out));
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    walk_stmt(i, out);
                }
                if let Some(c) = condition {
                    walk_expr(c, out);
                }
                if let Some(u) = update {
                    walk_expr(u, out);
                }
                body.iter().for_each(|s| walk_stmt(s, out));
            }
            Stmt::Labeled { body, .. } => walk_stmt(body, out),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                body.iter().for_each(|s| walk_stmt(s, out));
                if let Some(c) = catch {
                    c.body.iter().for_each(|s| walk_stmt(s, out));
                }
                if let Some(f) = finally {
                    f.iter().for_each(|s| walk_stmt(s, out));
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                walk_expr(discriminant, out);
                cases.iter().for_each(|case| {
                    if let Some(t) = &case.test {
                        walk_expr(t, out);
                    }
                    case.body.iter().for_each(|s| walk_stmt(s, out));
                });
            }
            _ => {}
        }
    }

    for s in &hir.init {
        walk_stmt(s, &mut out);
    }
    for f in &hir.functions {
        for s in &f.body {
            walk_stmt(s, &mut out);
        }
    }
    out
}
