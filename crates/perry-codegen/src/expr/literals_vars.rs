//! Literals, variables, update, DateNow.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, Result};
use perry_hir::types::Type as HirType;
use perry_hir::{BinaryOp, Expr, UpdateOp};

use crate::lower_string_method::lower_string_self_append;
use crate::nanbox::double_literal;
use crate::native_value::MaterializationReason;
use crate::type_analysis::{is_map_expr, is_set_expr, receiver_class_name};
use crate::types::{DOUBLE, I32, I64};

use super::{
    can_lower_expr_as_i32_in_current_region, emit_root_nanbox_store_on_block,
    emit_shadow_slot_clear, emit_shadow_slot_update_for_expr, emit_write_barrier,
    is_global_this_builtin_function_name, lower_expr, lower_expr_as_i32,
    lower_pod_local_reassignment, materialize_pod_local, nanbox_string_inline, FnCtx,
};

/// #1380: method names addressable on a `Set` instance, used by the
/// `typeof set.<name>` fold to report "function" (Set method values are
/// not materialized as real function objects). Includes the ES2024
/// composition methods.
fn is_set_method_name(name: &str) -> bool {
    matches!(
        name,
        "has"
            | "add"
            | "delete"
            | "clear"
            | "forEach"
            | "entries"
            | "values"
            | "keys"
            | "union"
            | "intersection"
            | "difference"
            | "symmetricDifference"
            | "isSubsetOf"
            | "isSupersetOf"
            | "isDisjointFrom"
    )
}

/// #1380: method names addressable on a `Map` instance, used by the
/// `typeof map.<name>` fold (same rationale as `is_set_method_name`).
fn is_map_method_name(name: &str) -> bool {
    matches!(
        name,
        "has" | "get" | "set" | "delete" | "clear" | "forEach" | "entries" | "values" | "keys"
    )
}

fn is_headers_method_name(name: &str) -> bool {
    matches!(
        name,
        "append"
            | "delete"
            | "entries"
            | "forEach"
            | "get"
            | "getSetCookie"
            | "has"
            | "keys"
            | "set"
            | "Symbol.iterator"
            | "@@iterator"
            | "values"
    )
}

fn is_headers_instance_method(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> bool {
    is_headers_method_name(property)
        && matches!(receiver_class_name(ctx, object).as_deref(), Some("Headers"))
        // #6003: a user-defined `class Headers` owns the receiver type —
        // its members are ordinary class members, not the native surface.
        && !ctx.classes.contains_key("Headers")
}

fn is_classic_stream_method_name(name: &str) -> bool {
    matches!(
        name,
        "on" | "addListener"
            | "once"
            | "prependListener"
            | "prependOnceListener"
            | "emit"
            | "listeners"
            | "rawListeners"
            | "eventNames"
            | "listenerCount"
            | "removeListener"
            | "off"
            | "removeAllListeners"
            | "setMaxListeners"
            | "getMaxListeners"
    )
}

fn is_classic_stream_instance_method(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> bool {
    if !is_classic_stream_method_name(property) {
        return false;
    }
    matches!(
        receiver_class_name(ctx, object).as_deref(),
        Some("Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough" | "Stream")
    )
}

fn fs_lchmod_callable_on_target(target_triple: &str) -> bool {
    let target = target_triple.to_ascii_lowercase();
    target.contains("darwin")
        || target.contains("macos")
        || target.contains("ios")
        || target.contains("freebsd")
        || target.contains("netbsd")
        || target.contains("openbsd")
        || target.contains("dragonfly")
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::Integer(i) => Ok(double_literal(*i as f64)),
        Expr::Number(f) => Ok(double_literal(*f)),
        // Booleans are NaN-boxed using TAG_TRUE/TAG_FALSE — both are
        // double bit patterns inside the NaN range, emitted as hex
        // literals (LLVM's `0x{16-hex}` form for non-finite doubles).
        Expr::Bool(b) => {
            let tag = if *b {
                crate::nanbox::TAG_TRUE
            } else {
                crate::nanbox::TAG_FALSE
            };
            Ok(double_literal(f64::from_bits(tag)))
        }
        // `undefined` and `null` lower to their NaN-tagged bit patterns.
        Expr::Undefined => Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))),
        Expr::Null => Ok(double_literal(f64::from_bits(crate::nanbox::TAG_NULL))),
        Expr::NewTarget => {
            if let Some(slot) = ctx.new_target_stack.last().cloned() {
                Ok(ctx.block().load(DOUBLE, &slot))
            } else {
                Ok(ctx.block().call(DOUBLE, "js_new_target_get", &[]))
            }
        }

        // `void <expr>` — evaluate the operand for side effects, return
        // undefined. Used both as `void 0` (a common idiom for `undefined`)
        // and `void (sideEffect = 42)` for discarding an assignment value.
        Expr::Void(operand) => {
            let _ = lower_expr(ctx, operand)?;
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // `typeof <expr>` — calls js_value_typeof which returns a runtime
        // string handle ("number", "string", "boolean", "undefined",
        // "object", "function"). The result is NaN-boxed with STRING_TAG.
        Expr::TypeOf(operand) => {
            // Issue #574: short-circuit known compile-time shapes that
            // `js_value_typeof` would misclassify because the runtime
            // representation collides with a different tag:
            //
            //   * Namespace ExternFuncRef (`import * as Lib from "./m"`)
            //     lowers as a TAG_TRUE sentinel → typeof reads "boolean".
            //     Emit "object" instead.
            //   * Class refs (local `Expr::ClassRef` and imported
            //     `Expr::ExternFuncRef` resolving via `class_ids`, plus
            //     the namespace-member class case `Lib.A`) lower as
            //     INT32-tagged class ids → typeof reads "number". Emit
            //     "function" to match JS spec for class objects.
            let typeof_short_circuit: Option<&'static str> = match operand.as_ref() {
                Expr::ExternFuncRef { name, .. } if ctx.namespace_imports.contains(name) => {
                    Some("object")
                }
                Expr::ExternFuncRef { name, .. } if ctx.class_ids.contains_key(name) => {
                    Some("function")
                }
                Expr::ClassRef(_) => Some("function"),
                Expr::NativeMethodCall {
                    module,
                    class_name: None,
                    object: Some(_),
                    method,
                    ..
                } if module == "Headers" && is_headers_method_name(method) => Some("function"),
                // Issue #623: native-module default-imports (`import process
                // from "node:process"`) lower as `NativeModuleRef`, which the
                // codegen represents as a `0.0` stub double. `js_value_typeof`
                // reads it as a number; per spec native-module bindings are
                // objects.
                Expr::NativeModuleRef(_) => Some("object"),
                // Issue #623: bare `typeof globalThis` — perry models the
                // global object as `GlobalGet(0)` lowering to `0.0`, same
                // misclassification.
                Expr::GlobalGet(_) => Some("object"),
                Expr::PropertyGet {
                    object, property, ..
                } => {
                    // #1380: `typeof set.has` / `typeof map.get` → "function".
                    // Set/Map methods aren't materialized as real function
                    // objects — a bare `set.has` read returns the (absent)
                    // data property, so `js_value_typeof` would report
                    // "undefined". The receiver type is known here via
                    // `is_set_expr`/`is_map_expr` (the same routing that makes
                    // `set.size` resolve to a number), so fold known method
                    // names to "function". Covers
                    // `process.allowedNodeEnvironmentFlags` (lowered to a Set)
                    // whose `.has`/`.size` callers feature-detect with typeof.
                    if (is_set_expr(ctx, object) && is_set_method_name(property))
                        || (is_map_expr(ctx, object) && is_map_method_name(property))
                    {
                        Some("function")
                    } else if is_headers_instance_method(ctx, object, property) {
                        Some("function")
                    } else if is_classic_stream_instance_method(ctx, object, property) {
                        Some("function")
                    } else if let Expr::ExternFuncRef { name, .. } = object.as_ref() {
                        if ctx.namespace_imports.contains(name)
                            && ctx.class_ids.contains_key(property)
                        {
                            Some("function")
                        } else {
                            None
                        }
                    } else if matches!(object.as_ref(), Expr::GlobalGet(_)) {
                        // Issue #623: `(globalThis as any).process` /
                        // `globalThis.console` — known Node globals that are
                        // objects in spec. The codegen lowers
                        // `globalThis.<name>` to a generic property read that
                        // produces a stub double; typeof would read "number"
                        // without this short-circuit. Function-shaped globals
                        // (Buffer, Promise, URL, etc.) intentionally fall
                        // through so `typeof Buffer === "function"` keeps
                        // working through the existing class-ref path.
                        //
                        // lodash followup: built-in constructors exposed on
                        // globalThis (`Array`, `Object`, `Function`, …) now
                        // also lower the bare PropertyGet to a real value
                        // (a backing-object pointer materialized by
                        // `js_get_global_this`'s singleton populator).
                        // Without the typeof short-circuit, `typeof
                        // globalThis.Array` would read "object" (the value
                        // is a real pointer); spec says "function". Math /
                        // JSON / Reflect stay "object" — they're namespaces,
                        // not constructors.
                        match property.as_str() {
                            "process" | "console" | "globalThis" | "performance" | "navigator"
                            | "crypto" | "localStorage" | "sessionStorage" => Some("object"),
                            "Math" | "JSON" | "Reflect" | "Atomics" | "Intl" | "Temporal" => {
                                Some("object")
                            }
                            n if is_global_this_builtin_function_name(n) => Some("function"),
                            _ => None,
                        }
                    } else if let Expr::NativeModuleRef(module) = object.as_ref() {
                        // #1343: `typeof <nativeModule>.<member>` (e.g.
                        // `typeof crypto.randomBytes`, `typeof process.cwd`).
                        // A method is only addressable through the call-
                        // dispatch arms, so reading it as a plain value yields
                        // the module's `0.0` stub and `js_value_typeof` reports
                        // "undefined"/"number". Short-circuit only methods and
                        // exported classes to "function". Properties fall
                        // through (`None`): their value is materialized for
                        // real, so the generic typeof already reports the right
                        // primitive/object kind (`process.pid` → "number",
                        // `os.EOL` → "string", `crypto.constants` → "object").
                        if matches!(module.as_str(), "fs" | "node:fs")
                            && matches!(property.as_str(), "lchmod" | "lchmodSync")
                            && !fs_lchmod_callable_on_target(ctx.target_triple)
                        {
                            None
                        } else {
                            match perry_api_manifest::module_has_symbol(module, property) {
                                Some(e)
                                    if matches!(
                                        e.kind,
                                        perry_api_manifest::ApiKind::Method { .. }
                                            | perry_api_manifest::ApiKind::Class
                                    ) =>
                                {
                                    Some("function")
                                }
                                _ => None,
                            }
                        }
                    } else {
                        // Refs #915 (gap 2 from #899): `typeof C.staticMethod`
                        // where `C` is `Expr::ClassRef` or a `LocalGet`
                        // aliased to a class. Without this fold, the
                        // generic PropertyGet path returns `undefined`
                        // for static methods (the runtime `class_has_own_method`
                        // checks the prototype vtable, not the static
                        // method registry), so `typeof Cls.pipe` reported
                        // `"undefined"` instead of `"function"`. The actual
                        // dispatch fix lives in `lower_call.rs`'s ClassRef
                        // static-method arm — but a typeof read isn't a
                        // call, so it needs its own fold here.
                        let cls_opt: Option<String> = match object.as_ref() {
                            Expr::ClassRef(cls_name) => Some(cls_name.clone()),
                            Expr::LocalGet(id) => ctx
                                .local_id_to_name
                                .get(id)
                                .and_then(|name| ctx.local_class_aliases.get(name).cloned()),
                            _ => None,
                        };
                        if let Some(cls) = cls_opt {
                            // Walk own static methods + extends chain.
                            let mut cur = Some(cls);
                            let mut found = false;
                            while let Some(c) = cur {
                                if let Some(class_info) = ctx.classes.get(&c) {
                                    if class_info
                                        .static_methods
                                        .iter()
                                        .any(|m| m.name == *property)
                                    {
                                        found = true;
                                        break;
                                    }
                                    cur = class_info.extends_name.clone();
                                } else {
                                    break;
                                }
                            }
                            if found {
                                Some("function")
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                }
                _ => None,
            };
            if let Some(s) = typeof_short_circuit {
                let idx = ctx.strings.intern(s);
                let entry = ctx.strings.entry(idx);
                let handle_global = format!("@{}", entry.handle_global);
                return Ok(ctx.block().load(DOUBLE, &handle_global));
            }
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_value_typeof", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }

        // String literals are pre-allocated at module init via the
        // StringPool's hoisting strategy (see `crate::strings`). At the use
        // site we just load the cached NaN-boxed handle from the pool's
        // `.handle` global. ONE instruction, no per-use allocation.
        Expr::String(s) => {
            let idx = ctx.strings.intern(s);
            let entry = ctx.strings.entry(idx);
            // Clone the global name out so we don't keep `entry` borrowed
            // across the call to `ctx.block()` (which mutably borrows
            // `ctx.func`, distinct from `ctx.strings` but the borrow checker
            // sees `entry` as borrowing `ctx`).
            let handle_global = format!("@{}", entry.handle_global);
            Ok(ctx.block().load(DOUBLE, &handle_global))
        }

        // WTF-8 string literals (contain lone surrogates U+D800..U+DFFF).
        // Same hoisting strategy as Expr::String, but initialized via
        // js_string_from_wtf8_bytes which sets STRING_FLAG_HAS_LONE_SURROGATES.
        Expr::WtfString(bytes) => {
            let idx = ctx.strings.intern_wtf8(bytes);
            let entry = ctx.strings.entry(idx);
            let handle_global = format!("@{}", entry.handle_global);
            Ok(ctx.block().load(DOUBLE, &handle_global))
        }

        // -------- Variables --------
        // LocalGet lookup order:
        //   1. Closure captures (when lowering inside a closure body) →
        //      runtime js_closure_get_capture_bits(this_closure, idx)
        //   2. Function-local alloca slots
        //   3. Module-level globals
        //
        // This lets closures read captured outer variables, regular
        // functions read their own params/lets, and any function read
        // module-scope `let`s (the ones in `hir.init` at top level).
        Expr::LocalGet(id) => {
            if ctx.pod_records.contains_key(id) {
                return materialize_pod_local(ctx, *id, MaterializationReason::PodMaterialization);
            }
            // Captured by closure (from outer scope):
            if let Some(&capture_idx) = ctx.closure_captures.get(id) {
                let closure_ptr = ctx
                    .current_closure_ptr
                    .clone()
                    .ok_or_else(|| anyhow!("captured local but no current_closure_ptr"))?;
                let idx_str = capture_idx.to_string();
                // If the captured id is a boxed var, the capture slot holds a
                // raw box pointer. Read the capture, extract the box pointer,
                // and deref via js_box_get_bits.
                if ctx.boxed_vars.contains(id) {
                    let blk = ctx.block();
                    let box_ptr = blk.call(
                        I64,
                        "js_closure_get_capture_bits",
                        &[(I64, &closure_ptr), (I32, &idx_str)],
                    );
                    let bits = blk.call(I64, "js_box_get_bits", &[(I64, &box_ptr)]);
                    return Ok(blk.bitcast_i64_to_double(&bits));
                }
                let bits = ctx.block().call(
                    I64,
                    "js_closure_get_capture_bits",
                    &[(I64, &closure_ptr), (I32, &idx_str)],
                );
                return Ok(ctx.block().bitcast_i64_to_double(&bits));
            }
            // Boxed local in enclosing function: load the slot (box
            // pointer), deref via js_box_get_bits.
            //
            // #6369: never for a MODULE GLOBAL. Its storage is the
            // `@perry_global_*` cell, which holds the VALUE — `Stmt::Let`
            // stores it there directly and never allocates a box — so a
            // box deref here would reinterpret e.g. an array pointer as a
            // box pointer. `LocalSet` and `Update` (below) already carry
            // this exclusion; the read path was the odd one out, and it
            // only stayed latent because a module global normally has no
            // `ctx.locals` slot to find. The packed-loop invariant-global
            // read cache installs exactly such a slot (`loops.rs` aliases
            // the global into `ctx.locals` for the duration of the loop),
            // so any module global that landed in the module-wide boxed
            // union — which every closure inherits wholesale — was read
            // back as garbage (`NaN`) there.
            if ctx.boxed_vars.contains(id) && !ctx.module_globals.contains_key(id) {
                if let Some(slot) = ctx.locals.get(id).cloned() {
                    let blk = ctx.block();
                    let box_ptr = blk.load(I64, &slot);
                    let bits = blk.call(I64, "js_box_get_bits", &[(I64, &box_ptr)]);
                    return Ok(blk.bitcast_i64_to_double(&bits));
                }
            }
            if let Some(slot) = ctx.locals.get(id).cloned() {
                // Issue #48: prefer the i32 slot for int32-stable locals so
                // LLVM can promote the alloca to an i32 SSA value and skip the
                // double round-trip. The double slot is still maintained (for
                // closures or escape sites) but mem2reg + DSE will eliminate
                // it when the i32 path covers every read.
                if let Some(i32_slot) = ctx.i32_counter_slots.get(id).cloned() {
                    let i = ctx.block().load(I32, &i32_slot);
                    let v = if ctx.unsigned_i32_locals.contains(id) {
                        ctx.block().uitofp(I32, &i, DOUBLE)
                    } else {
                        ctx.block().sitofp(I32, &i, DOUBLE)
                    };
                    return Ok(v);
                }
                Ok(ctx.block().load(DOUBLE, &slot))
            } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                let g_ref = format!("@{}", global_name);
                Ok(ctx.block().load(DOUBLE, &g_ref))
            } else {
                // Soft fallback: the HIR sometimes carries stale
                // local references that don't correspond to any
                // declared param/let/global in the current scope
                // (curry-style nested closures, async transformer
                // intermediate ids, etc.). Return undefined so
                // compilation succeeds without fabricating a numeric 0.
                Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
            }
        }

        // `total = expr` — store the new value into the local's alloca slot
        // and return it (matches JS semantics: assignment is an expression
        // whose value is the assigned value).
        //
        // SPECIAL FAST PATH: `x = x + y` where `x` is a string-typed local.
        // Uses
        // `js_string_append` (in-place for refcount=1 unique owners)
        // instead of `js_string_concat` (always allocates). For a 10K-
        // iteration `str = str + "a"` build loop, this turns O(n²) total
        // work into O(n) and is the difference between 700 ms and 200 ms
        // on bench_string_ops.
        Expr::LocalSet(id, value) => {
            super::invalidate_local_write_facts(ctx, *id);
            super::record_local_value_alias_for_write(ctx, *id, value.as_ref());
            if let Some(v) = lower_pod_local_reassignment(ctx, *id, value)? {
                super::record_native_arena_owner_assignment(ctx, *id, value.as_ref());
                return Ok(v);
            }
            // Detect the `x = x + y` self-append pattern.
            // The fast path requires a plain alloca slot in `ctx.locals` —
            // module globals (use `@global` loads), closure captures (use
            // `js_closure_{get,set}_capture_bits`), and boxed vars (use
            // `js_box_set_bits` through a heap cell) all need different store
            // mechanics, so they fall through to the regular `LocalSet`
            // path below. Issue #319: without the `ctx.locals.contains_key`
            // / closure_captures / boxed_vars guards, a closure-captured
            // string-typed local that does `s = s + t` aborted codegen
            // with `string self-append: local N not in scope` because the
            // helper's `ctx.locals.get(id)` lookup whiffed.
            if matches!(ctx.local_types.get(id), Some(HirType::String))
                && !ctx.module_globals.contains_key(id)
                && !ctx.closure_captures.contains_key(id)
                && !ctx.boxed_vars.contains(id)
                && ctx.locals.contains_key(id)
            {
                if let Expr::Binary {
                    op: BinaryOp::Add,
                    left,
                    right,
                } = value.as_ref()
                {
                    if let Expr::LocalGet(left_id) = left.as_ref() {
                        if left_id == id {
                            let v = lower_string_self_append(ctx, *id, right)?;
                            emit_shadow_slot_update_for_expr(ctx, *id, &v, value);
                            super::record_native_arena_owner_assignment(ctx, *id, value.as_ref());
                            return Ok(v);
                        }
                    }
                }
            }

            // Issue #49: integer-arithmetic fast path. When the target has an
            // i32 slot (i.e. it's in `integer_locals`) and every leaf of the
            // rhs can be sourced in i32, emit the whole rhs as i32 and store
            // directly to the i32 slot. Skips the `sitofp→...fadd/fmul...→
            // fptosi` round-trip that the fp path otherwise forces on every
            // `acc = acc + byte * k` iteration. The double slot is maintained
            // via one sitofp per write so non-int readers (e.g. `acc / K`)
            // still see the current value.
            if let Some(i32_slot) = ctx.i32_counter_slots.get(id).cloned() {
                if !ctx.closure_captures.contains_key(id)
                    && !(ctx.boxed_vars.contains(id) && !ctx.module_globals.contains_key(id))
                    && can_lower_expr_as_i32_in_current_region(ctx, value)
                {
                    let v_i32 = lower_expr_as_i32(ctx, value)?;
                    let unsigned_i32 = ctx.unsigned_i32_locals.contains(id);
                    let blk = ctx.block();
                    blk.store(I32, &v_i32, &i32_slot);
                    let v_dbl = if unsigned_i32 {
                        blk.uitofp(I32, &v_i32, DOUBLE)
                    } else {
                        blk.sitofp(I32, &v_i32, DOUBLE)
                    };
                    if let Some(slot) = ctx.locals.get(id).cloned() {
                        ctx.block().store(DOUBLE, &v_dbl, &slot);
                    } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                        let g_ref = format!("@{}", global_name);
                        // GC_STORE_AUDIT(ROOT): module global slot is registered as a mutable GC root.
                        emit_root_nanbox_store_on_block(ctx.block(), &v_dbl, &g_ref);
                    }
                    if let Some(slot_idx) = ctx.shadow_slot_map.get(id).copied() {
                        emit_shadow_slot_clear(ctx, slot_idx);
                    }
                    super::record_native_arena_owner_assignment(ctx, *id, value.as_ref());
                    super::record_int_facts_for_local_set(ctx, *id, value);
                    return Ok(v_dbl);
                }
            }

            let v = lower_expr(ctx, value)?;
            // Closure captures first (write through the runtime), then
            // locals, then module globals.
            if let Some(&capture_idx) = ctx.closure_captures.get(id) {
                let closure_ptr = ctx
                    .current_closure_ptr
                    .clone()
                    .ok_or_else(|| anyhow!("captured local set but no current_closure_ptr"))?;
                let idx_str = capture_idx.to_string();
                // Boxed captured var: read the box pointer from the
                // capture slot, then js_box_set_bits to update the shared
                // cell. Do NOT overwrite the capture slot — it holds
                // the box pointer, not the value.
                if ctx.boxed_vars.contains(id) {
                    let blk = ctx.block();
                    let box_ptr = blk.call(
                        I64,
                        "js_closure_get_capture_bits",
                        &[(I64, &closure_ptr), (I32, &idx_str)],
                    );
                    let v_bits = blk.bitcast_double_to_i64(&v);
                    blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &v_bits)]);
                    // Gen-GC Phase C2: barrier — box is the parent.
                    emit_write_barrier(ctx, &box_ptr, &v_bits);
                } else {
                    let v_bits = ctx.block().bitcast_double_to_i64(&v);
                    ctx.block().call_void(
                        "js_closure_set_capture_bits",
                        &[(I64, &closure_ptr), (I32, &idx_str), (I64, &v_bits)],
                    );
                    // Gen-GC Phase C2: barrier — closure is the parent.
                    emit_write_barrier(ctx, &closure_ptr, &v_bits);
                }
            } else if ctx.boxed_vars.contains(id) && !ctx.module_globals.contains_key(id) {
                // Box path — only for non-global locals. Module globals
                // have their own shared storage and don't need boxing.
                // Without the !module_globals guard, closures that
                // modify a module-level variable would silently skip
                // the store (ctx.locals doesn't have the global's slot).
                if let Some(slot) = ctx.locals.get(id).cloned() {
                    let blk = ctx.block();
                    let box_ptr = blk.load(I64, &slot);
                    let v_bits = blk.bitcast_double_to_i64(&v);
                    blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &v_bits)]);
                    // Gen-GC Phase C2: barrier — box is the parent (mirror the
                    // captured-box path above; an old box can else miss a young
                    // object/string/array value).
                    emit_write_barrier(ctx, &box_ptr, &v_bits);
                }
            } else if let Some(slot) = ctx.locals.get(id).cloned() {
                ctx.block().store(DOUBLE, &v, &slot);
                // Gen-GC Phase A sub-phase 3b: mirror pointer-typed
                // writes into the shadow frame. See stmt.rs::Let
                // for the allocation-site mirror; LocalSet is the
                // reassignment-site mirror.
                emit_shadow_slot_update_for_expr(ctx, *id, &v, value);
                // Mirror to the parallel i32 slot allocated for int32-stable
                // locals (issue #48). Without this, the i32 slot would go
                // stale on every `sum = (sum + i) | 0` write.
                // Use fptosi→i64 + trunc→i32 to safely handle unsigned values
                // (e.g. xorshift state `s = ... >>> 0` where double > INT32_MAX).
                if let Some(i32_slot) = ctx.i32_counter_slots.get(id).cloned() {
                    let v_i64 = ctx.block().fptosi(DOUBLE, &v, crate::types::I64);
                    let v_i32 = ctx.block().trunc(crate::types::I64, &v_i64, I32);
                    ctx.block().store(I32, &v_i32, &i32_slot);
                }
            } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                let g_ref = format!("@{}", global_name);
                // GC_STORE_AUDIT(ROOT): module global slot is registered as a mutable GC root.
                emit_root_nanbox_store_on_block(ctx.block(), &v, &g_ref);
            }
            super::record_native_arena_owner_assignment(ctx, *id, value.as_ref());
            if ctx.buffer_view_slots.contains_key(id)
                || matches!(
                    value.as_ref(),
                    Expr::BufferAlloc { .. } | Expr::BufferAllocUnsafe(_) | Expr::Uint8ArrayNew(_)
                )
            {
                super::update_buffer_view_for_assignment(ctx, *id, value, &v);
            }
            super::record_int_facts_for_local_set(ctx, *id, value);
            // Soft fallback: drop the store on the floor for missing
            // locals. See LocalGet for the rationale.
            Ok(v)
        }

        // `i++` / `++i` / `i--` / `--i`. Postfix returns the OLD value,
        // prefix returns the NEW value. Closure captures, locals, then
        // module globals.
        Expr::Update { id, op, prefix } => {
            super::invalidate_local_write_facts(ctx, *id);
            // Spec ToNumeric: `x++`/`++x` coerce the operand (ToNumber on
            // bool/null/string/object-with-valueOf, or BigInt passthrough)
            // before the add/sub, and the *returned* value (postfix) is the
            // coerced numeric, not the original boxed operand. Statically-
            // integer loop counters already hold a real f64 and skip the call
            // to keep the hot path a single `fadd`. The slow path routes
            // through `js_to_numeric`/`js_numeric_step` so a BigInt operand
            // stays a BigInt (`let i = 10n; i++` → `11n`, not the Number `11`
            // which would make a later `i + 87n` throw a mixed-type
            // TypeError; test262 BigInt/prototype/toString/a-z).
            let needs_numeric_coerce =
                !ctx.integer_locals.contains(id) && !ctx.unsigned_i32_locals.contains(id);
            let is_increment_arg = match op {
                UpdateOp::Increment => "1",
                UpdateOp::Decrement => "0",
            };
            let coerce_old = |blk: &mut crate::block::LlBlock, raw: &str| -> String {
                if needs_numeric_coerce {
                    blk.call(DOUBLE, "js_to_numeric", &[(DOUBLE, raw)])
                } else {
                    raw.to_string()
                }
            };
            let step_new = |blk: &mut crate::block::LlBlock, old_num: &str| -> String {
                if needs_numeric_coerce {
                    blk.call(
                        DOUBLE,
                        "js_numeric_step",
                        &[(DOUBLE, old_num), (I32, is_increment_arg)],
                    )
                } else {
                    match op {
                        UpdateOp::Increment => blk.fadd(old_num, "1.0"),
                        UpdateOp::Decrement => blk.fsub(old_num, "1.0"),
                    }
                }
            };
            // Closure capture path: runtime get + add/sub + runtime set.
            if let Some(&capture_idx) = ctx.closure_captures.get(id) {
                let closure_ptr = ctx
                    .current_closure_ptr
                    .clone()
                    .ok_or_else(|| anyhow!("captured local update but no current_closure_ptr"))?;
                let idx_str = capture_idx.to_string();
                // Boxed captured var: deref box bits, modify, store back.
                if ctx.boxed_vars.contains(id) {
                    let blk = ctx.block();
                    let box_ptr = blk.call(
                        I64,
                        "js_closure_get_capture_bits",
                        &[(I64, &closure_ptr), (I32, &idx_str)],
                    );
                    let old_bits = blk.call(I64, "js_box_get_bits", &[(I64, &box_ptr)]);
                    let old = blk.bitcast_i64_to_double(&old_bits);
                    let old = coerce_old(blk, &old);
                    let new = step_new(blk, &old);
                    let new_bits = blk.bitcast_double_to_i64(&new);
                    blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &new_bits)]);
                    // Gen-GC Phase C2: `++`/`--` on a BigInt yields a heap
                    // pointer via js_numeric_step — barrier the box parent.
                    emit_write_barrier(ctx, &box_ptr, &new_bits);
                    return Ok(if *prefix { new } else { old });
                }
                let old_bits = ctx.block().call(
                    I64,
                    "js_closure_get_capture_bits",
                    &[(I64, &closure_ptr), (I32, &idx_str)],
                );
                let old = ctx.block().bitcast_i64_to_double(&old_bits);
                let blk = ctx.block();
                let old = coerce_old(blk, &old);
                let new = step_new(blk, &old);
                let new_bits = blk.bitcast_double_to_i64(&new);
                blk.call_void(
                    "js_closure_set_capture_bits",
                    &[(I64, &closure_ptr), (I32, &idx_str), (I64, &new_bits)],
                );
                // Gen-GC Phase C2: barrier — closure is the parent (BigInt
                // `++`/`--` can store a young heap pointer).
                emit_write_barrier(ctx, &closure_ptr, &new_bits);
                return Ok(if *prefix { new } else { old });
            }
            // Boxed enclosing-scope var: load slot (box ptr), deref,
            // increment, box_set_bits. Skip for module globals (they
            // have their own shared storage).
            if ctx.boxed_vars.contains(id) && !ctx.module_globals.contains_key(id) {
                if let Some(slot) = ctx.locals.get(id).cloned() {
                    let blk = ctx.block();
                    let box_ptr = blk.load(I64, &slot);
                    let old_bits = blk.call(I64, "js_box_get_bits", &[(I64, &box_ptr)]);
                    let old = blk.bitcast_i64_to_double(&old_bits);
                    let old = coerce_old(blk, &old);
                    let new = step_new(blk, &old);
                    let new_bits = blk.bitcast_double_to_i64(&new);
                    blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &new_bits)]);
                    // Gen-GC Phase C2: barrier — box is the parent (BigInt
                    // `++`/`--` can store a young heap pointer).
                    emit_write_barrier(ctx, &box_ptr, &new_bits);
                    return Ok(if *prefix { new } else { old });
                }
            }
            let (storage, storage_is_root) = if let Some(slot) = ctx.locals.get(id).cloned() {
                (slot, false)
            } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                (format!("@{}", global_name), true)
            } else {
                // Soft fallback: silently increment a throwaway value.
                return Ok(double_literal(0.0));
            };
            let blk = ctx.block();
            let old = blk.load(DOUBLE, &storage);
            let old = coerce_old(blk, &old);
            let new = step_new(blk, &old);
            if storage_is_root {
                // Module globals are registered mutable GC roots and route
                // through the root helper; the raw store below is stack-only.
                emit_root_nanbox_store_on_block(blk, &new, &storage);
            } else {
                // GC_STORE_AUDIT(STACK): update writes a function-local alloca;
                // module globals use the root helper.
                blk.store(DOUBLE, &new, &storage);
            }
            // Keep the parallel i32 counter slot in sync (if active).
            // This costs one `add i32, 1` per iteration but saves a
            // `fptosi double → i32` on every IndexGet/IndexSet use.
            if let Some(i32_slot) = ctx.i32_counter_slots.get(id).cloned() {
                let blk = ctx.block();
                let old_i32 = blk.load(I32, &i32_slot);
                let delta = match op {
                    UpdateOp::Increment => "1",
                    UpdateOp::Decrement => "-1",
                };
                let new_i32 = blk.add(I32, &old_i32, delta);
                blk.store(I32, &new_i32, &i32_slot);
            }
            super::record_int_facts_for_update(ctx, *id, *op);
            Ok(if *prefix { new } else { old })
        }

        // `Date.now()` — special HIR variant that lowers to a single FFI
        // call returning a `double` (milliseconds since UNIX epoch as
        // produced by `js_date_now` in `perry-runtime/src/date.rs`).
        Expr::DateNow => Ok(ctx.block().call(DOUBLE, "js_date_now", &[])),

        // -------- Arithmetic --------
        // String concatenation (Phase B): if Add receives operands where
        // either side is statically a string, route through string concat.
        // - both strings → `lower_string_concat` (inline bitcast+and unbox)
        // - one string + one non-string → `lower_string_coerce_concat`
        //   (the non-string side passes through `js_jsvalue_to_string`
        //   which dispatches on the NaN tag at runtime)
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
