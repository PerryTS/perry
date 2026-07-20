//! V8-fallback-module interop helpers + static-class-name resolution
//! (extracted from `expr.rs`, issue #1098). Pure move — no logic changes.

use perry_hir::Expr;

use super::FnCtx;
use crate::types::{DOUBLE, I64, PTR};

/// Issue #678: resolve the actual symbol-suffix for an imported name.
///
/// Re-export renames like `export { default as render } from './render.js'`
/// mean the consumer sees `render` while the origin module emits
/// `perry_fn_<origin>__default`. The `import_function_origin_names` map
/// records this rename so every `perry_fn_<src>__<suffix>` construction
/// site can pick the right suffix instead of the consumer-visible name.
///
/// Returns the override when present, falling back to `name` (the common
/// case where origin name == consumer name). Used by every codegen
/// path that builds a `perry_fn_<src>__<name>` extern symbol.
pub(crate) fn import_origin_suffix<'a>(
    origin_names: &'a std::collections::HashMap<String, String>,
    name: &'a str,
) -> &'a str {
    origin_names.get(name).map(String::as_str).unwrap_or(name)
}

/// Issue #5924 (companion to #678/#680): namespace-scoped variant of
/// `import_origin_suffix`.
///
/// `import_function_origin_names` is flat (keyed by bare member name), so
/// when two namespaces imported into the same file both have a member with
/// the same name and only one of them is a re-export rename, the rename's
/// origin-name override clobbers the other namespace's (correct, unrenamed)
/// suffix — `import { Effect, Context } from "effect"` broke
/// `Context.Service` because `Effect`'s own re-exported `Service` clobbered
/// the flat map first. Prefers the per-namespace override
/// (`(namespace_local, member_name)`) before falling through to the flat
/// map.
pub(crate) fn import_origin_suffix_ns<'a>(
    origin_names: &'a std::collections::HashMap<String, String>,
    namespace_origin_names: &'a std::collections::HashMap<(String, String), String>,
    namespace_local: &str,
    name: &'a str,
) -> &'a str {
    namespace_origin_names
        .get(&(namespace_local.to_string(), name.to_string()))
        .map(String::as_str)
        .unwrap_or_else(|| import_origin_suffix(origin_names, name))
}

/// Issue #678 followup: emit a `js_call_v8_export` bridge call for a name
/// that resolves to a V8-fallback (interpreted) module.
///
/// Materializes per-call-site rodata constants for the module specifier
/// and the export name (linker merges duplicates across translation units),
/// stack-allocates an f64 args array, and emits the runtime call. Returns
/// the SSA double register holding the NaN-boxed result.
///
/// Caller has already validated that `name` is in
/// `ctx.import_function_v8_specifiers`; this helper just emits the lowering.
pub(crate) fn emit_v8_export_call(
    ctx: &mut FnCtx<'_>,
    specifier: &str,
    export_name: &str,
    lowered_args: &[String],
) -> String {
    let idx = ctx.typed_parse_counter;
    ctx.typed_parse_counter += 1;
    let spec_global = format!("perry_v8_spec_{}", idx);
    let name_global = format!("perry_v8_name_{}", idx);
    let escape = |s: &str| -> String {
        let bytes = s.as_bytes();
        let mut lit = String::with_capacity(bytes.len() + 4);
        lit.push('c');
        lit.push('"');
        for &b in bytes {
            if (32..127).contains(&b) && b != b'"' && b != b'\\' {
                lit.push(b as char);
            } else {
                lit.push('\\');
                lit.push_str(&format!("{:02X}", b));
            }
        }
        lit.push_str("\\00\"");
        lit
    };
    let spec_bytes = specifier.len();
    let name_bytes = export_name.len();
    ctx.typed_parse_rodata.push(format!(
        "@{} = private unnamed_addr constant [{} x i8] {}",
        spec_global,
        spec_bytes + 1,
        escape(specifier)
    ));
    ctx.typed_parse_rodata.push(format!(
        "@{} = private unnamed_addr constant [{} x i8] {}",
        name_global,
        name_bytes + 1,
        escape(export_name)
    ));

    let argc = lowered_args.len();
    let alloca_count = if argc == 0 { 1 } else { argc };
    let blk = ctx.block();
    let argc_lit = format!("{}", argc);
    let spec_ptr = format!("@{}", spec_global);
    let name_ptr = format!("@{}", name_global);
    let spec_len_lit = format!("{}", spec_bytes);
    let name_len_lit = format!("{}", name_bytes);

    // Stack-allocate the args buffer (zero-len → still need a pointer; an
    // `alloca [1 x double]` is well-formed in LLVM and never dereferenced
    // because argc=0 in that branch of the runtime).
    let args_slot = blk.fresh_reg();
    blk.emit_raw(format!(
        "{} = alloca [{} x double], align 8",
        args_slot, alloca_count
    ));
    for (i, v) in lowered_args.iter().enumerate() {
        let slot = blk.fresh_reg();
        blk.emit_raw(format!(
            "{} = getelementptr inbounds [{} x double], ptr {}, i64 0, i64 {}",
            slot, alloca_count, args_slot, i
        ));
        // GC_STORE_AUDIT(STACK): V8 interop args are transient stack alloca slots.
        blk.emit_raw(format!("store double {}, ptr {}, align 8", v, slot));
    }

    ctx.pending_declares.push((
        "js_call_v8_export".to_string(),
        DOUBLE,
        vec![PTR, I64, PTR, I64, PTR, I64],
    ));
    let blk = ctx.block();
    blk.call(
        DOUBLE,
        "js_call_v8_export",
        &[
            (PTR, &spec_ptr),
            (I64, &spec_len_lit),
            (PTR, &name_ptr),
            (I64, &name_len_lit),
            (PTR, &args_slot),
            (I64, &argc_lit),
        ],
    )
}

/// Issue #818 (Effect.succeed pattern): emit a
/// `js_call_v8_member_method(spec, member, method, args)` bridge call for a
/// named V8 import used as a static-method receiver — `Effect.succeed(42)`
/// where `Effect` is `import { Effect } from 'effect'`. The V8 module's
/// top-level export `Effect` is itself a namespace-shaped object whose
/// `.succeed` is the actual function; the existing `emit_v8_export_call`
/// would mistakenly try to invoke `effect.succeed(...)` at the module root.
pub(crate) fn emit_v8_member_method_call(
    ctx: &mut FnCtx<'_>,
    specifier: &str,
    member: &str,
    method: &str,
    lowered_args: &[String],
) -> String {
    let idx = ctx.typed_parse_counter;
    ctx.typed_parse_counter += 1;
    let spec_global = format!("perry_v8_mspec_{}", idx);
    let member_global = format!("perry_v8_mmember_{}", idx);
    let method_global = format!("perry_v8_mmethod_{}", idx);
    let escape = |s: &str| -> String {
        let bytes = s.as_bytes();
        let mut lit = String::with_capacity(bytes.len() + 4);
        lit.push('c');
        lit.push('"');
        for &b in bytes {
            if (32..127).contains(&b) && b != b'"' && b != b'\\' {
                lit.push(b as char);
            } else {
                lit.push('\\');
                lit.push_str(&format!("{:02X}", b));
            }
        }
        lit.push_str("\\00\"");
        lit
    };
    let spec_bytes = specifier.len();
    let member_bytes = member.len();
    let method_bytes = method.len();
    ctx.typed_parse_rodata.push(format!(
        "@{} = private unnamed_addr constant [{} x i8] {}",
        spec_global,
        spec_bytes + 1,
        escape(specifier)
    ));
    ctx.typed_parse_rodata.push(format!(
        "@{} = private unnamed_addr constant [{} x i8] {}",
        member_global,
        member_bytes + 1,
        escape(member)
    ));
    ctx.typed_parse_rodata.push(format!(
        "@{} = private unnamed_addr constant [{} x i8] {}",
        method_global,
        method_bytes + 1,
        escape(method)
    ));

    let argc = lowered_args.len();
    let alloca_count = if argc == 0 { 1 } else { argc };
    let blk = ctx.block();
    let argc_lit = format!("{}", argc);
    let spec_ptr = format!("@{}", spec_global);
    let member_ptr = format!("@{}", member_global);
    let method_ptr = format!("@{}", method_global);
    let spec_len_lit = format!("{}", spec_bytes);
    let member_len_lit = format!("{}", member_bytes);
    let method_len_lit = format!("{}", method_bytes);

    let args_slot = blk.fresh_reg();
    blk.emit_raw(format!(
        "{} = alloca [{} x double], align 8",
        args_slot, alloca_count
    ));
    for (i, v) in lowered_args.iter().enumerate() {
        let slot = blk.fresh_reg();
        blk.emit_raw(format!(
            "{} = getelementptr inbounds [{} x double], ptr {}, i64 0, i64 {}",
            slot, alloca_count, args_slot, i
        ));
        // GC_STORE_AUDIT(STACK): V8 interop args are transient stack alloca slots.
        blk.emit_raw(format!("store double {}, ptr {}, align 8", v, slot));
    }

    ctx.pending_declares.push((
        "js_call_v8_member_method".to_string(),
        DOUBLE,
        vec![PTR, I64, PTR, I64, PTR, I64, PTR, I64],
    ));
    let blk = ctx.block();
    blk.call(
        DOUBLE,
        "js_call_v8_member_method",
        &[
            (PTR, &spec_ptr),
            (I64, &spec_len_lit),
            (PTR, &member_ptr),
            (I64, &member_len_lit),
            (PTR, &method_ptr),
            (I64, &method_len_lit),
            (PTR, &args_slot),
            (I64, &argc_lit),
        ],
    )
}

/// If `callee` is a `new`-target whose class name is statically
/// known, return that name. Used by the `Expr::NewDynamic` lowering
/// to reroute statically-resolvable shapes to the regular `lower_new`
/// path. Returns `None` for any callee that needs runtime dispatch
/// (locals, conditionals with non-classy arms, computed expressions).
///
/// Recognized shapes:
///   - `Expr::ClassRef(name)` — class identifier referenced as a value
///     (the lowering at `crates/perry-hir/src/lower.rs::ast::Expr::Ident`
///     turns class names referenced as values into ClassRef so they
///     can flow through generic Expr slots without losing the class
///     identity).
///   - `Expr::PropertyGet { object: GlobalGet(_), property }` — a
///     property access on the global object, e.g. `globalThis.WebSocket`
///     or `window.Date`. The `globalThis.X` form is what the parser
///     emits for `new globalThis.WebSocket(url)` (mango uses this for
///     the websocket helper in `_wsOpen`).
///   - `Expr::PropertyGet { object: LocalGet(ns_id), property }` where
///     `ns_id` is a namespace import local (`import * as ns from 'm';
///     new ns.Foo()`). The local id is mapped to its name via
///     `ctx.local_id_to_name`, then checked against
///     `ctx.namespace_imports`. The property name is returned as the
///     class name; the rest of the lower_new path resolves it via the
///     usual `ctx.classes` lookup, which contains imported classes
///     under their original (un-namespaced) names.
fn is_global_object_expr(expr: &Expr) -> bool {
    match expr {
        Expr::GlobalGet(_) => true,
        Expr::PropertyGet {
            object, property, ..
        } if property == "globalThis" => is_global_object_expr(object),
        _ => false,
    }
}

pub(crate) fn try_static_class_name<'a>(callee: &'a Expr, ctx: &FnCtx<'_>) -> Option<&'a str> {
    match callee {
        Expr::ClassRef(name) => Some(name.as_str()),
        // Refs #486: `new _X()` where `_X` is the inner self-binding name of
        // a class expression (e.g. `var X = class _X { ... new _X() ... }`)
        // lowers to `NewDynamic { callee: ExternFuncRef("_X") }` because the
        // inner name isn't a real outer-scope identifier — the HIR walker
        // can't resolve it to anything but an unknown extern. Recognize it
        // here by checking the per-module class_ids table, which codegen has
        // already populated with the inner-name → same-id mapping at
        // compile_module entry. Without this, the call falls through to the
        // empty-object placeholder path with class_id=0 and method dispatch
        // breaks on the resulting instance.
        Expr::ExternFuncRef { name, .. } if ctx.class_ids.contains_key(name) => Some(name.as_str()),
        Expr::PropertyGet {
            object, property, ..
        } => {
            if is_global_object_expr(object.as_ref()) {
                return Some(property.as_str());
            }
            // Namespace import: `import * as ns from 'm'; new ns.Foo()`.
            // The namespace object surfaces either as `LocalGet(id)` (mapped to
            // its name via `local_id_to_name`) or directly as
            // `ExternFuncRef { name: "ns" }` (the HIR's `ast::Expr::Ident`
            // lowering at `crates/perry-hir/src/lower.rs` lifts a namespace
            // identifier this way). In both forms the property access becomes
            // `PropertyGet { object, property: "Foo" }`.
            //
            // #4698: only treat the member as a class when `property` actually
            // names a known class. A namespace can equally re-export a
            // closure-valued `const` (zod's `$ZodCheckMinLength =
            // $constructor(...)`); for those, returning `Some(property)` routes
            // through `lower_new`, which finds no matching class and emits an
            // empty-object placeholder whose constructor body never runs (so
            // `Object.defineProperty(this, …)` writes are lost). Falling
            // through to `None` instead lets the NewDynamic dispatcher read the
            // member as a closure value and run it via
            // `js_new_function_construct` (which also intercepts the builtin
            // global thunks, so re-exported builtins keep working).
            let object_is_namespace = match object.as_ref() {
                Expr::LocalGet(id) => ctx
                    .local_id_to_name
                    .get(id)
                    .is_some_and(|name| ctx.namespace_imports.contains(name)),
                Expr::ExternFuncRef { name, .. } => ctx.namespace_imports.contains(name),
                _ => false,
            };
            if object_is_namespace && ctx.classes.contains_key(property.as_str()) {
                return Some(property.as_str());
            }
            None
        }
        _ => None,
    }
}
