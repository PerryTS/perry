//! StaticFieldGet..NativeModuleRef (class meta + getters).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, bail, Result};
#[allow(unused_imports)]
use perry_hir::{BinaryOp, CompareOp, Expr, UnaryOp, UpdateOp};
#[allow(unused_imports)]
use perry_types::Type as HirType;

#[allow(unused_imports)]
use crate::lower_call::{lower_call, lower_native_method_call, lower_new};
#[allow(unused_imports)]
use crate::lower_conditional::{lower_conditional, lower_logical, lower_truthy};
#[allow(unused_imports)]
use crate::lower_string_method::{
    flatten_string_add_chain, lower_string_coerce_concat, lower_string_concat,
    lower_string_concat_chain, lower_string_self_append,
};
#[allow(unused_imports)]
use crate::nanbox::{double_literal, POINTER_MASK_I64};
#[allow(unused_imports)]
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

#[allow(unused_imports)]
use super::{
    buffer_alias_metadata_suffix, can_lower_expr_as_i32, emit_layout_note_slot_on_block,
    emit_shadow_slot_clear, emit_shadow_slot_update_for_expr, emit_string_literal_global,
    emit_v8_export_call, emit_v8_member_method_call, emit_write_barrier,
    emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
    extract_array_of_object_shape, i32_bool_to_nanbox, import_origin_suffix,
    is_global_this_builtin_function_name, is_global_this_builtin_name, is_known_finite,
    lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32,
    lower_index_set_fast, lower_js_args_array, lower_object_literal, lower_stream_super_init,
    lower_url_string_getter, nanbox_bigint_inline, nanbox_pointer_inline,
    nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array, try_flat_const_2d_int,
    try_lower_flat_const_index_get, try_match_channel_reduction, try_static_class_name,
    unbox_str_handle, unbox_to_i64, variant_name, ChannelReduction, FlatConstInfo, FnCtx,
    I18nLowerCtx,
};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::StaticFieldGet {
            class_name,
            field_name,
        } => {
            let key = (class_name.clone(), field_name.clone());
            if let Some(global_name) = ctx.static_field_globals.get(&key).cloned() {
                let g_ref = format!("@{}", global_name);
                Ok(ctx.block().load(DOUBLE, &g_ref))
            } else {
                Ok(double_literal(0.0))
            }
        }
        Expr::StaticFieldSet {
            class_name,
            field_name,
            value,
        } => {
            let v = lower_expr(ctx, value)?;
            let key = (class_name.clone(), field_name.clone());
            if let Some(global_name) = ctx.static_field_globals.get(&key).cloned() {
                let g_ref = format!("@{}", global_name);
                ctx.block().store(DOUBLE, &v, &g_ref);
            }
            // v0.5.747: also register the static field in the runtime
            // CLASS_DYNAMIC_PROPS side-table so dynamic-dispatch reads
            // (when the class ref is in an Any-typed local) find it.
            // Refs #420 / #618 followup.
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                let idx = ctx.strings.intern(field_name);
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
                        (DOUBLE, &v),
                    ],
                );
            }
            Ok(v)
        }
        // Issue #711: dynamic parent-class registration for `class X
        // extends fn(...)` shapes. Evaluate the extends expression and
        // call `js_register_class_parent_dynamic(child_cid, value)`,
        // which extracts a class_id from the value (ClassRef payload
        // for INT32-tagged, ObjectHeader.class_id for POINTER-tagged)
        // and wires the parent edge into CLASS_REGISTRY. No-op if the
        // value carries no class_id — preserves the parentless
        // baseline rather than throwing during module init.
        Expr::RegisterClassParentDynamic {
            class_name,
            parent_expr,
        } => {
            let val = lower_expr(ctx, parent_expr)?;
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                if class_id != 0 {
                    let cid_str = class_id.to_string();
                    ctx.block().call_void(
                        "js_register_class_parent_dynamic",
                        &[(crate::types::I32, &cid_str), (DOUBLE, &val)],
                    );
                }
            }
            // Yield undefined — this expression is always wrapped in
            // `Stmt::Expr` for its side effect; the return value isn't
            // observable to user code.
            Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)))
        }
        // Issue #894: `static [Symbol.for("k")] = init` inside a
        // class expression returned from a factory function. Emitted
        // by HIR lowering as a `Sequence([…, RegisterClassStaticSymbol,
        // ClassRef])` so each factory invocation re-registers the
        // (class_id, sym_key) → value entry. Without this, the
        // registration would only happen at module-init time when
        // referenced free variables may not yet be assigned, and
        // `isSchema(C)` (which checks `TypeId in C`) returns false on
        // a freshly-returned class.
        Expr::RegisterClassStaticSymbol {
            class_name,
            key_expr,
            value_expr,
        } => {
            let key_v = lower_expr(ctx, key_expr)?;
            let val_v = lower_expr(ctx, value_expr)?;
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                if class_id != 0 {
                    let cid_str = class_id.to_string();
                    ctx.block().call_void(
                        "js_class_register_static_symbol",
                        &[
                            (crate::types::I32, &cid_str),
                            (DOUBLE, &key_v),
                            (DOUBLE, &val_v),
                        ],
                    );
                }
            }
            Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)))
        }
        // Issue #711 part 2: `<expr>.prototype = <expr>` pattern.
        // Calls `js_set_function_prototype(func, proto)`, which (when
        // func is a closure and proto is an object) allocates a
        // synthetic class id and binds the proto object as that
        // class's vtable source. Method dispatch later consults
        // CLASS_PROTOTYPE_OBJECTS to resolve methods.
        Expr::SetFunctionPrototype { func, proto } => {
            let func_val = lower_expr(ctx, func)?;
            let proto_val = lower_expr(ctx, proto)?;
            // Discard the returned synthetic class id — it's stored in
            // the runtime side-table keyed by func_val and consulted
            // later by `js_register_class_parent_dynamic`. User code
            // gets the assigned value (proto_val) as the expression
            // result, matching JS semantics for `x.foo = bar`.
            let _ = ctx.block().call(
                crate::types::I32,
                "js_set_function_prototype",
                &[(DOUBLE, &func_val), (DOUBLE, &proto_val)],
            );
            Ok(proto_val)
        }
        // Issue #838: `<Class>.prototype.<method> = <fn>` and the
        // aliased `let p = <Class>.prototype; p.<method> = <fn>`
        // shape. HIR recognises the assignment pattern and lowers it
        // here; codegen emits `js_register_prototype_method(class_id,
        // name_ptr, name_len, value)` so the runtime stores the
        // closure into a per-class side-table consulted at dispatch
        // time. The expression yields the closure value to match
        // JS-spec `x.foo = bar`. If the class isn't in
        // `ctx.class_ids` (cross-module imported class) we fall back
        // to a generic field-set on `<Class>.prototype` so the value
        // at least lands on the prototype proxy — the importer side
        // typically owns the registration anyway.
        Expr::RegisterPrototypeMethod {
            class_name,
            method_name,
            value,
        } => {
            let val_double = lower_expr(ctx, value)?;
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                let key_idx = ctx.strings.intern(method_name);
                let key_bytes_global = format!("@{}", ctx.strings.entry(key_idx).bytes_global);
                let key_len = ctx.strings.entry(key_idx).byte_len.to_string();
                ctx.block().call_void(
                    "js_register_prototype_method",
                    &[
                        (crate::types::I32, &class_id.to_string()),
                        (PTR, &key_bytes_global),
                        (I64, &key_len),
                        (DOUBLE, &val_double),
                    ],
                );
            }
            Ok(val_double)
        }
        // Issue #838 followup (b): the prototype's owner is a function
        // declaration (Babel's `var Foo = function(){ function Foo(){…};
        // Foo.prototype.x = fn; return Foo; }()`, also dayjs's minified
        // form). Hand both the closure value and the method name to the
        // runtime helper — it allocates (or reuses) a synthetic class id
        // keyed by the closure's NaN-boxed bits and stores the method
        // on `CLASS_PROTOTYPE_METHODS[synthetic_cid]`. The paired
        // `new <FuncRef>(args)` lowering below stamps the same id on
        // the instance so dispatch finds the method via the regular
        // `(*obj).class_id` walk.
        Expr::RegisterFunctionPrototypeMethod {
            func,
            method_name,
            value,
        } => {
            let func_double = lower_expr(ctx, func)?;
            let val_double = lower_expr(ctx, value)?;
            let key_idx = ctx.strings.intern(method_name);
            let key_bytes_global = format!("@{}", ctx.strings.entry(key_idx).bytes_global);
            let key_len = ctx.strings.entry(key_idx).byte_len.to_string();
            let _ = ctx.block().call(
                crate::types::I32,
                "js_register_function_prototype_method",
                &[
                    (DOUBLE, &func_double),
                    (PTR, &key_bytes_global),
                    (I64, &key_len),
                    (DOUBLE, &val_double),
                ],
            );
            Ok(val_double)
        }
        // Read side of #838 followup (b): `<funcDecl>.prototype.<name>`
        // (Ident or computed-string-literal form) lowered into a direct
        // lookup of the prototype-method side-table. Returns the closure
        // value stored at registration time, or `undefined` if no method
        // by that name was registered. Pre-fix this would fall through
        // to a generic PropertyGet on a `Function.prototype` object that
        // never materialised (so the read was always `undefined`,
        // making `typeof Foo.prototype.method` come back `'undefined'`
        // even though `(new Foo()).method` correctly reached the
        // registered closure via the dispatch path).
        Expr::GetFunctionPrototypeMethod { func, method_name } => {
            let func_double = lower_expr(ctx, func)?;
            let key_idx = ctx.strings.intern(method_name);
            let key_bytes_global = format!("@{}", ctx.strings.entry(key_idx).bytes_global);
            let key_len = ctx.strings.entry(key_idx).byte_len.to_string();
            Ok(ctx.block().call(
                DOUBLE,
                "js_get_function_prototype_method",
                &[
                    (DOUBLE, &func_double),
                    (PTR, &key_bytes_global),
                    (I64, &key_len),
                ],
            ))
        }
        // `static [Symbol.for("k")] = "v"` — register in the runtime's
        // class-static-symbol side table. Refs #420 (drizzle).
        Expr::ClassStaticSymbolSet {
            class_name,
            key,
            value,
        } => {
            let key_v = lower_expr(ctx, key)?;
            let val_v = lower_expr(ctx, value)?;
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                let cid_str = class_id.to_string();
                ctx.block().call_void(
                    "js_class_register_static_symbol",
                    &[
                        (crate::types::I32, &cid_str),
                        (DOUBLE, &key_v),
                        (DOUBLE, &val_v),
                    ],
                );
            }
            Ok(val_v)
        }
        // Issue #894: when `NativeModuleRef` reaches this fallback path
        // (i.e. its parent isn't one of the dedicated fast-paths above —
        // for example it's the *return value* of a CJS-wrap synthesized
        // `require()` call, then stashed in a local and member-accessed
        // later: `const { EventEmitter } = require('node:events')` →
        // `Let { id: 6, init: Call(require, "node:events") }` followed by
        // `Let { id: 7, init: PropertyGet { LocalGet(6), "EventEmitter" } }`),
        // pre-fix the value lowered to the literal `0.0`. `0.0` is plain
        // f64 zero (not the NaN-boxed `undefined` tag), so a subsequent
        // `PropertyGet { LocalGet(6), "X" }` slow-pathed into
        // `js_object_get_field_by_name` with a null receiver and returned
        // `undefined` — and a then-chained `PropertyGet { LocalGet(7),
        // "prototype" }` on that `undefined` tripped the spec
        // "Cannot read properties of undefined (reading 'prototype')"
        // throw (pino's `lib/proto.js` exact shape).
        //
        // Materialize a real NATIVE_MODULE_CLASS_ID-tagged ObjectHeader
        // here so the downstream property-by-name path routes through the
        // namespace's NATIVE_MODULE_CLASS_ID arm in
        // `js_object_get_field_by_name` — that consults
        // `get_native_module_constant` and `is_native_module_callable_export`
        // exactly as the direct-NativeModuleRef fast path does. The two
        // paths now converge: `import * as fs from "node:fs"; fs.constants`
        // (direct AST shape, fast-path at line 3615) and `const fs =
        // require("node:fs"); fs.constants` (call-result shape, fallback
        // path here) both produce a real namespace object.
        Expr::NativeModuleRef(name) => {
            let mod_idx = ctx.strings.intern(name);
            let mod_bytes_global = format!("@{}", ctx.strings.entry(mod_idx).bytes_global);
            let mod_len_str = name.len().to_string();
            Ok(ctx.block().call(
                DOUBLE,
                "js_create_native_module_namespace",
                &[(PTR, &mod_bytes_global), (I64, &mod_len_str)],
            ))
        }

        // ObjectRest is the `...rest` capture in destructuring:
        // `const { a, b, ...rest } = obj` — `rest` must be a clone of
        // `obj` with keys `a`/`b` stripped. We build an exclude-keys
        // array of NaN-boxed strings and call `js_object_rest`, which
        // returns a fresh object pointer that we re-NaN-box.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
