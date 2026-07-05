//! InstanceOf..JsonParseWithReviver.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, Result};
#[allow(unused_imports)]
use perry_hir::{BinaryOp, CompareOp, Expr, UnaryOp, UpdateOp, WithSetFallback};
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
use crate::nanbox::{double_literal, i64_literal, POINTER_MASK_I64};
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
    emit_root_nanbox_store_on_block, emit_shadow_slot_bind_for_local, emit_shadow_slot_clear,
    emit_shadow_slot_update_for_expr, emit_string_literal_global, emit_v8_export_call,
    emit_v8_member_method_call, emit_write_barrier, emit_write_barrier_slot_on_block,
    expr_is_known_non_pointer_shadow_value, extract_array_of_object_shape, i32_bool_to_nanbox,
    import_origin_suffix, is_global_this_builtin_function_name, is_global_this_builtin_name,
    is_known_finite, lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32,
    lower_index_set_fast, lower_js_args_array, lower_object_literal, lower_stream_super_init,
    lower_url_string_getter, nanbox_bigint_inline, nanbox_pointer_inline,
    nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array, try_flat_const_2d_int,
    try_lower_flat_const_index_get, try_match_channel_reduction, try_static_class_name,
    unbox_str_handle, unbox_to_i64, variant_name, ChannelReduction, FlatConstInfo, FnCtx,
    I18nLowerCtx,
};

/// Reserved runtime class id for a built-in constructor usable as a class
/// heritage (`class S extends Array {}`). Used to register the subclass →
/// built-in parent edge so `new S() instanceof Array` walks the class chain
/// and matches. The ids MUST stay in sync with the `instanceof <Builtin>`
/// match in `lower_instanceof` (this file) and the per-id branches in
/// perry-runtime/src/object/instanceof.rs. Returns `None` for names without a
/// reserved id (those can't be subclassed with working `instanceof` yet).
pub(crate) fn builtin_parent_reserved_class_id(name: &str) -> Option<u32> {
    Some(match name {
        "Error" => 0xFFFF0001,
        "TypeError" => 0xFFFF0010,
        "RangeError" => 0xFFFF0011,
        "ReferenceError" => 0xFFFF0012,
        "SyntaxError" => 0xFFFF0013,
        "AggregateError" => 0xFFFF0014,
        "EvalError" => 0xFFFF0015,
        "URIError" => 0xFFFF0016,
        "Date" => 0xFFFF0020,
        "RegExp" => 0xFFFF0021,
        "Map" => 0xFFFF0022,
        "Set" => 0xFFFF0023,
        "Array" => 0xFFFF0024,
        "ArrayBuffer" => 0xFFFF0025,
        "DataView" => 0xFFFF002B,
        "WeakMap" => 0xFFFF002C,
        "WeakSet" => 0xFFFF002D,
        "Promise" => 0xFFFF0027,
        "Number" => 0xFFFF00D0,
        "String" => 0xFFFF00D1,
        "Boolean" => 0xFFFF00D2,
        "BigInt" => 0xFFFF00D3,
        "Symbol" => 0xFFFF00D4,
        "Int8Array" => 0xFFFF0030,
        "Uint8Array" => 0xFFFF0004,
        "Int16Array" => 0xFFFF0032,
        "Uint16Array" => 0xFFFF0033,
        "Int32Array" => 0xFFFF0034,
        "Uint32Array" => 0xFFFF0035,
        "Float32Array" => 0xFFFF0036,
        "Float64Array" => 0xFFFF0037,
        "Uint8ClampedArray" => 0xFFFF0038,
        "BigInt64Array" => 0xFFFF0039,
        "BigUint64Array" => 0xFFFF003A,
        "Function" => 0xFFFF00F0,
        _ => return None,
    })
}

fn emit_with_key(ctx: &mut FnCtx<'_>, property: &str) -> (String, String) {
    let key_idx = ctx.strings.intern(property);
    let key_entry = ctx.strings.entry(key_idx);
    let key_global = format!("@{}", key_entry.handle_global);
    let key_box = ctx.block().load(DOUBLE, &key_global);
    let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
    let key_raw = ctx.block().and(I64, &key_bits, POINTER_MASK_I64);
    (key_box, key_raw)
}

fn store_prelowered_local(ctx: &mut FnCtx<'_>, id: u32, value: &str) -> Result<String> {
    super::invalidate_local_write_facts(ctx, id);
    if let Some(&capture_idx) = ctx.closure_captures.get(&id) {
        let closure_ptr = ctx
            .current_closure_ptr
            .clone()
            .ok_or_else(|| anyhow!("captured with-fallback set but no current_closure_ptr"))?;
        let idx_str = capture_idx.to_string();
        if ctx.boxed_vars.contains(&id) {
            let blk = ctx.block();
            let box_ptr = blk.call(
                I64,
                "js_closure_get_capture_bits",
                &[(I64, &closure_ptr), (I32, &idx_str)],
            );
            let value_bits = blk.bitcast_double_to_i64(value);
            blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &value_bits)]);
            emit_write_barrier(ctx, &box_ptr, &value_bits);
        } else {
            let value_bits = ctx.block().bitcast_double_to_i64(value);
            ctx.block().call_void(
                "js_closure_set_capture_bits",
                &[(I64, &closure_ptr), (I32, &idx_str), (I64, &value_bits)],
            );
            emit_write_barrier(ctx, &closure_ptr, &value_bits);
        }
    } else if ctx.boxed_vars.contains(&id) && !ctx.module_globals.contains_key(&id) {
        if let Some(slot) = ctx.locals.get(&id).cloned() {
            let blk = ctx.block();
            let box_ptr = blk.load(I64, &slot);
            let value_bits = blk.bitcast_double_to_i64(value);
            blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &value_bits)]);
            emit_write_barrier(ctx, &box_ptr, &value_bits);
        }
    } else if let Some(slot) = ctx.locals.get(&id).cloned() {
        ctx.block().store(DOUBLE, value, &slot);
        if let Some(slot_idx) = ctx.shadow_slot_map.get(&id).copied() {
            emit_shadow_slot_bind_for_local(ctx, id);
            let value_bits = ctx.block().bitcast_double_to_i64(value);
            ctx.block().call_void(
                "js_shadow_slot_set",
                &[(I32, &slot_idx.to_string()), (I64, &value_bits)],
            );
        }
        if let Some(i32_slot) = ctx.i32_counter_slots.get(&id).cloned() {
            let value_i64 = ctx.block().fptosi(DOUBLE, value, I64);
            let value_i32 = ctx.block().trunc(I64, &value_i64, I32);
            ctx.block().store(I32, &value_i32, &i32_slot);
        }
    } else if let Some(global_name) = ctx.module_globals.get(&id).cloned() {
        let g_ref = format!("@{}", global_name);
        emit_root_nanbox_store_on_block(ctx.block(), value, &g_ref);
    }
    Ok(value.to_string())
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::WithGet {
            object,
            property,
            fallback,
        } => {
            let obj = lower_expr(ctx, object)?;
            let (_key_box, key_raw) = emit_with_key(ctx, property);
            let has = ctx.block().call(
                I32,
                "js_with_has_binding",
                &[(DOUBLE, &obj), (I64, &key_raw)],
            );
            let has_bool = ctx.block().icmp_ne(I32, &has, "0");

            let hit_idx = ctx.new_block("with.get.hit");
            let miss_idx = ctx.new_block("with.get.miss");
            let merge_idx = ctx.new_block("with.get.merge");
            let hit_label = ctx.block_label(hit_idx);
            let miss_label = ctx.block_label(miss_idx);
            let merge_label = ctx.block_label(merge_idx);
            ctx.block().cond_br(&has_bool, &hit_label, &miss_label);

            ctx.current_block = hit_idx;
            let hit = ctx.block().call(
                DOUBLE,
                "js_with_get_binding",
                &[(DOUBLE, &obj), (I64, &key_raw)],
            );
            let hit_after = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&merge_label);
            }

            ctx.current_block = miss_idx;
            let miss = lower_expr(ctx, fallback)?;
            let miss_after = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&merge_label);
            }

            ctx.current_block = merge_idx;
            Ok(ctx
                .block()
                .phi(DOUBLE, &[(&hit, &hit_after), (&miss, &miss_after)]))
        }
        Expr::WithSet {
            object,
            property,
            value,
            fallback,
            strict,
        } => {
            let obj = lower_expr(ctx, object)?;
            let (key_box, key_raw) = emit_with_key(ctx, property);
            // HasBinding probe AFTER the RHS evaluates — matches V8/node
            // (`with (o) { var x = delete o.x; }` writes the hoisted var,
            // not o.x — test262 variable/binding-resolution.js judges
            // against node's order, not the spec's resolve-reference-first).
            let value_reg = lower_expr(ctx, value)?;
            let had = ctx.block().call(
                I32,
                "js_with_has_binding",
                &[(DOUBLE, &obj), (I64, &key_raw)],
            );
            let had_bool = ctx.block().icmp_ne(I32, &had, "0");

            let hit_idx = ctx.new_block("with.set.hit");
            let miss_idx = ctx.new_block("with.set.miss");
            let merge_idx = ctx.new_block("with.set.merge");
            let hit_label = ctx.block_label(hit_idx);
            let miss_label = ctx.block_label(miss_idx);
            let merge_label = ctx.block_label(merge_idx);
            ctx.block().cond_br(&had_bool, &hit_label, &miss_label);

            ctx.current_block = hit_idx;
            let strict_i32 = if *strict { "1" } else { "0" };
            let hit = ctx.block().call(
                DOUBLE,
                "js_with_set_binding",
                &[
                    (DOUBLE, &obj),
                    (I64, &key_raw),
                    (DOUBLE, &value_reg),
                    (I32, strict_i32),
                ],
            );
            let hit_after = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&merge_label);
            }

            ctx.current_block = miss_idx;
            let miss = match fallback {
                WithSetFallback::Local(id) | WithSetFallback::SloppyImplicit(id) => {
                    store_prelowered_local(ctx, *id, &value_reg)?
                }
                WithSetFallback::ThrowReferenceError => ctx.block().call(
                    DOUBLE,
                    "js_throw_reference_error_unresolvable_assignment",
                    &[(DOUBLE, &key_box)],
                ),
                WithSetFallback::ThrowConstAssignment => ctx.block().call(
                    DOUBLE,
                    "js_throw_type_error_const_assignment",
                    &[(DOUBLE, &key_box)],
                ),
                WithSetFallback::Ignore => value_reg.clone(),
            };
            let miss_after = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&merge_label);
            }

            ctx.current_block = merge_idx;
            Ok(ctx
                .block()
                .phi(DOUBLE, &[(&hit, &hit_after), (&miss, &miss_after)]))
        }
        Expr::InstanceOf {
            expr: e,
            ty,
            ty_expr,
        } => {
            let v = lower_expr(ctx, e)?;
            // v0.5.749: dynamic dispatch when the type is a runtime
            // expression (function arg, local holding a class ref).
            // The runtime helper `js_instanceof_dynamic` extracts the
            // class_id from the INT32 NaN-tag and walks the chain.
            // Refs #420 / #618 followup.
            if let Some(ty_e) = ty_expr {
                let ty_v = lower_expr(ctx, ty_e)?;
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_instanceof_dynamic",
                    &[(DOUBLE, &v), (DOUBLE, &ty_v)],
                ));
            }
            if let Some((submod_key, exported_name)) = ctx.import_function_node_submodule.get(ty) {
                if submod_key == "diagnostics_channel"
                    && matches!(exported_name.as_str(), "Channel" | "BoundedChannel")
                {
                    let install_sym = crate::nm_install::nm_submod_install_symbol(submod_key);
                    let submod_label = emit_string_literal_global(ctx, submod_key);
                    let name_label = emit_string_literal_global(ctx, exported_name);
                    let submod_len = submod_key.len();
                    let name_len = exported_name.len();
                    let blk = ctx.block();
                    if let Some(s) = install_sym {
                        blk.call_void(s, &[]);
                    }
                    let ty_v = blk.call(
                        DOUBLE,
                        "js_node_submodule_export_as_function",
                        &[
                            (PTR, &submod_label),
                            (I32, &submod_len.to_string()),
                            (PTR, &name_label),
                            (I32, &name_len.to_string()),
                        ],
                    );
                    return Ok(blk.call(
                        DOUBLE,
                        "js_instanceof_dynamic",
                        &[(DOUBLE, &v), (DOUBLE, &ty_v)],
                    ));
                }
            }
            // A known non-constructor built-in namespace RHS (`Math`, `JSON`,
            // `Reflect`, `Atomics`) has no [[Call]]/[[Construct]], so
            // `x instanceof Math` throws a TypeError rather than folding to
            // `false`. These never map to a real class id, so without this they
            // would take the `cid == 0` → `js_instanceof(_, 0)` → `false` path.
            // The LHS `v` was already lowered above (its side effects run), so
            // just emit the throwing helper. Only fires for a bare identifier
            // RHS (no member/dynamic `ty_expr`) that isn't shadowed by a user
            // binding — a user `const Math = class {}` lands in `class_ids`.
            if matches!(ty.as_str(), "Math" | "JSON" | "Reflect" | "Atomics")
                && !ctx.class_ids.contains_key(ty)
            {
                return Ok(ctx
                    .block()
                    .call(DOUBLE, "js_instanceof_noncallable_rhs", &[]));
            }
            // Built-in Error subclasses have reserved CLASS_ID_* constants
            // in the runtime (see crates/perry-runtime/src/error.rs). Map
            // them by name here so `e instanceof TypeError` works even
            // though there's no user class definition.
            let imported_from_fs = ctx
                .imported_class_sources
                .get(ty)
                .map(|source| source.strip_prefix("node:").unwrap_or(source) == "fs")
                .unwrap_or(false);
            let imported_from_net = ctx
                .imported_class_sources
                .get(ty)
                .map(|source| source.strip_prefix("node:").unwrap_or(source) == "net")
                .unwrap_or(false);
            let cid = match ty.as_str() {
                "Error" => 0xFFFF0001u32,
                "TypeError" => 0xFFFF0010u32,
                "RangeError" => 0xFFFF0011u32,
                "ReferenceError" => 0xFFFF0012u32,
                "SyntaxError" => 0xFFFF0013u32,
                "AggregateError" => 0xFFFF0014u32,
                "EvalError" | "globalThis.EvalError" => 0xFFFF0015u32,
                "URIError" | "globalThis.URIError" => 0xFFFF0016u32,
                // Uint8Array / Buffer — runtime detects these via a
                // thread-local buffer registry (see buffer.rs). The
                // TextEncoder path registers its ArrayHeader result
                // in that same registry so `encoded instanceof Uint8Array`
                // returns true.
                "Uint8Array" | "Buffer" => 0xFFFF0004u32,
                // Other %TypedArray% kinds (#3148). The runtime resolves the
                // actual kind via TYPED_ARRAY_REGISTRY + class_id_for_kind in
                // instanceof.rs; these reserved ids must match the
                // CLASS_ID_* constants in perry-runtime/src/typedarray.rs.
                "Int8Array" => 0xFFFF0030u32,
                "Int16Array" => 0xFFFF0032u32,
                "Uint16Array" => 0xFFFF0033u32,
                "Int32Array" => 0xFFFF0034u32,
                "Uint32Array" => 0xFFFF0035u32,
                "Float32Array" => 0xFFFF0036u32,
                "Float64Array" => 0xFFFF0037u32,
                "Uint8ClampedArray" => 0xFFFF0038u32,
                "BigInt64Array" => 0xFFFF0039u32,
                "BigUint64Array" => 0xFFFF003Au32,
                "Float16Array" => 0xFFFF003Bu32,
                // Built-in JS types: Date, RegExp, Map, Set. The runtime
                // detects these via per-type registries (or, for Date,
                // by checking that the value is a finite f64 timestamp).
                "Date" => 0xFFFF0020u32,
                "RegExp" => 0xFFFF0021u32,
                "Map" => 0xFFFF0022u32,
                "Set" => 0xFFFF0023u32,
                // `Array` — runtime detects via GC_TYPE_ARRAY at obj-8.
                "Array" => 0xFFFF0024u32,
                "Number" => 0xFFFF00D0u32,
                "String" => 0xFFFF00D1u32,
                "Boolean" => 0xFFFF00D2u32,
                "BigInt" => 0xFFFF00D3u32,
                "Symbol" => 0xFFFF00D4u32,
                // `ArrayBuffer` — runtime detects BufferHeader storage marked
                // with Perry's ArrayBuffer side registry.
                "ArrayBuffer" => 0xFFFF0025u32,
                // WeakMap / WeakSet: real instances match via a runtime probe
                // (CLASS_ID_WEAKMAP/CLASS_ID_WEAKSET in weakref.rs, #5834) —
                // see the matching arm in perry-runtime/src/object/instanceof.rs.
                // DataView has no such probe yet (returns false independently),
                // but this reserved id still lets a `class S extends DataView {}`
                // subclass instance match via the class-chain walk. Refs
                // class/subclass-builtins/subclass-{WeakMap,WeakSet,DataView}.
                "DataView" => 0xFFFF002Bu32,
                "WeakMap" => 0xFFFF002Cu32,
                "WeakSet" => 0xFFFF002Du32,
                // `Blob` — stream consumers allocate a scoped Blob-shaped
                // ObjectHeader tagged with this reserved class id.
                "Blob" => 0xFFFF0026u32,
                // `Promise` — runtime detects via GC_TYPE_PROMISE because
                // Promise values are raw promise allocations, not ObjectHeader
                // instances with a class_id field.
                "Promise" => 0xFFFF0027u32,
                // WHATWG fetch types. Like Blob/streams these are pointer-tagged
                // small-int handles; the runtime resolves them via the stdlib
                // fetch kind-probe (`res instanceof Response`, etc.).
                "Response" => 0xFFFF0028u32,
                "Request" => 0xFFFF0029u32,
                "Headers" => 0xFFFF002Au32,
                // #1545: Web Streams. Handles are numeric ids; the runtime
                // resolves these via the stdlib stream-kind probe rather than
                // the class chain (`ts.readable instanceof ReadableStream`,
                // `rs.pipeThrough(ts) instanceof ReadableStream`, …).
                "ReadableStream" => 0xFFFF0060u32,
                "WritableStream" => 0xFFFF0061u32,
                "TransformStream" => 0xFFFF0062u32,
                // node:stream/web codec stream constructors are heap
                // ObjectHeader instances with runtime-owned class IDs.
                "TextEncoderStream" => 0x7FFFFF30u32,
                "TextDecoderStream" => 0x7FFFFF31u32,
                "CompressionStream" => 0x7FFFFF32u32,
                "DecompressionStream" => 0x7FFFFF33u32,
                // node:perf_hooks entry classes. Runtime classifies the
                // shaped entry objects returned by performance.mark/measure.
                // #3871: Performance / PerformanceObserverEntryList /
                // PerformanceResourceTiming moved off 0x87/0x88/0x86 (which
                // collided with fs Dirent/ReadStream/Dir) to 0x8E/0x8F/0x8D.
                // Keep in sync with perry-runtime/src/perf_hooks.rs.
                "Performance" => 0xFFFF008Eu32,
                "PerformanceEntry" => 0xFFFF0080u32,
                "PerformanceMark" => 0xFFFF0081u32,
                "PerformanceMeasure" => 0xFFFF0082u32,
                "PerformanceObserverEntryList" => 0xFFFF008Fu32,
                "PerformanceResourceTiming" => 0xFFFF008Du32,
                "Console" => 0xFFFF0083u32,
                // Temporal reference types. A Temporal value is a NaN-boxed
                // brand-tagged cell; the runtime resolves these reserved ids via
                // a brand-kind probe in object/instanceof.rs. Keep in sync with
                // perry-runtime/src/temporal/mod.rs (CLASS_ID_TEMPORAL_*).
                "Temporal.Duration" => 0xFFFF0200u32,
                "Temporal.Instant" => 0xFFFF0201u32,
                "Temporal.PlainDate" => 0xFFFF0202u32,
                "Temporal.PlainTime" => 0xFFFF0203u32,
                "Temporal.PlainDateTime" => 0xFFFF0204u32,
                "Temporal.PlainYearMonth" => 0xFFFF0205u32,
                "Temporal.PlainMonthDay" => 0xFFFF0206u32,
                "Temporal.ZonedDateTime" => 0xFFFF0207u32,
                "Event" | "globalThis.Event" => 0xFFFF2403u32,
                "CustomEvent" | "globalThis.CustomEvent" => 0xFFFF2404u32,
                "DOMException" | "globalThis.DOMException" => 0xFFFF2405u32,
                // node:fs constructor exports. Keep these ids in sync with
                // perry-runtime/src/fs/mod.rs and instanceof.rs.
                "fs.Dir" => 0xFFFF0086u32,
                "Dir" if imported_from_fs => 0xFFFF0086u32,
                "fs.Dirent" => 0xFFFF0087u32,
                "Dirent" if imported_from_fs => 0xFFFF0087u32,
                "fs.ReadStream" | "fs.FileReadStream" => 0xFFFF0088u32,
                "ReadStream" | "FileReadStream" if imported_from_fs => 0xFFFF0088u32,
                "fs.WriteStream" | "fs.FileWriteStream" => 0xFFFF0089u32,
                "WriteStream" | "FileWriteStream" if imported_from_fs => 0xFFFF0089u32,
                "fs.Stats" => 0xFFFF008Au32,
                "Stats" if imported_from_fs => 0xFFFF008Au32,
                "fs.Utf8Stream" => 0xFFFF008Bu32,
                "Utf8Stream" if imported_from_fs => 0xFFFF008Bu32,
                // node:net `Stream` is an alias for `Socket`. Both are native
                // small-handle values, so runtime probes the net socket map.
                "net.Socket" | "net.Stream" => 0xFFFF00B4u32,
                "Socket" | "Stream" if imported_from_net => 0xFFFF00B4u32,
                "ReadStream" | "tty.ReadStream" => 0xFFFF0084u32,
                "WriteStream" | "tty.WriteStream" => 0xFFFF0085u32,
                "SecureContext" | "tls.SecureContext" => 0xFFFF00B5u32,
                "WASI" | "wasi.WASI" => 0xFFFF00B2u32,
                "Crypto" => 0xFFFF00C0u32,
                "SubtleCrypto" => 0xFFFF00C1u32,
                "CryptoKey" => 0xFFFF00C2u32,
                // `Object` — every non-primitive matches per ECMAScript;
                // reserved id mapped in the runtime. Pre-#585 this fell
                // into the `cid = 0` fallback and matched accidentally
                // because the runtime's direct-class-id check returned
                // true on `0 == 0`. The #585 fix gates `class_id == 0`
                // → false, so `{} instanceof Object` would otherwise
                // regress; thread a real id through here instead.
                "Object" => 0xFFFF0050u32,
                // `Function` — every callable value (function declaration,
                // expression, arrow, method, bound function, native handle,
                // built-in constructor) is `instanceof Function`. The runtime
                // resolves this reserved id by testing callability rather than
                // walking a class chain. Keep in sync with
                // perry-runtime/src/object/instanceof.rs (CLASS_ID_FUNCTION).
                "Function" => 0xFFFF00F0u32,
                _ => ctx.class_ids.get(ty).copied().unwrap_or_else(|| {
                    // Keep in sync with perry-runtime/src/object/instanceof.rs.
                    let classic_stream_cid = match ty.as_str() {
                        "Stream" => Some(0xFFFF0070u32),
                        "Readable" => Some(0xFFFF0071u32),
                        "Writable" => Some(0xFFFF0072u32),
                        "Duplex" => Some(0xFFFF0073u32),
                        "Transform" => Some(0xFFFF0074u32),
                        "PassThrough" => Some(0xFFFF0075u32),
                        _ => None,
                    };
                    if let Some(cid) = classic_stream_cid {
                        return cid;
                    }
                    let native_event_cid = match ty.as_str() {
                        // Keep in sync with perry-runtime/src/object/instanceof.rs.
                        "EventEmitter" => Some(0xFFFF0076u32),
                        "EventEmitterAsyncResource" => Some(0xFFFF0077u32),
                        _ => None,
                    };
                    if let Some(cid) = native_event_cid {
                        return cid;
                    }
                    // Issue #574: `b instanceof Lib.A` where Lib is a
                    // namespace import. The HIR captures the receiver
                    // as a dotted `ty` ("Lib.A") which `class_ids`
                    // doesn't have. Strip the namespace prefix and
                    // re-lookup by the bare class name when the prefix
                    // matches a known namespace import.
                    if let Some((ns, cls)) = ty.split_once('.') {
                        if ctx.namespace_imports.contains(ns) {
                            return ctx.class_ids.get(cls).copied().unwrap_or(0);
                        }
                    }
                    0
                }),
            };
            let cid_str = cid.to_string();
            Ok(ctx
                .block()
                .call(DOUBLE, "js_instanceof", &[(DOUBLE, &v), (I32, &cid_str)]))
        }

        // -------- delete obj.prop / delete obj["prop"] --------
        // Recognize the two common shapes:
        //   - PropertyGet { object, property: <static name> }
        //   - IndexGet { object, index: <string literal or local> }
        // Both lower to js_object_delete_field with the static or
        // dynamic key. Anything else is a no-op stub returning true.
        Expr::Delete(operand) => {
            match operand.as_ref() {
                Expr::WithGet {
                    object,
                    property,
                    fallback,
                } => {
                    let obj = lower_expr(ctx, object)?;
                    let (_key_box, key_raw) = emit_with_key(ctx, property);
                    let has = ctx.block().call(
                        I32,
                        "js_with_has_binding",
                        &[(DOUBLE, &obj), (I64, &key_raw)],
                    );
                    let has_bool = ctx.block().icmp_ne(I32, &has, "0");

                    let hit_idx = ctx.new_block("with.delete.hit");
                    let miss_idx = ctx.new_block("with.delete.miss");
                    let merge_idx = ctx.new_block("with.delete.merge");
                    let hit_label = ctx.block_label(hit_idx);
                    let miss_label = ctx.block_label(miss_idx);
                    let merge_label = ctx.block_label(merge_idx);
                    ctx.block().cond_br(&has_bool, &hit_label, &miss_label);

                    ctx.current_block = hit_idx;
                    let deleted = ctx.block().call(
                        I32,
                        "js_with_delete_binding",
                        &[(DOUBLE, &obj), (I64, &key_raw)],
                    );
                    let deleted_bit = ctx.block().icmp_ne(I32, &deleted, "0");
                    let hit_tagged = ctx.block().select(
                        crate::types::I1,
                        &deleted_bit,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    let hit = ctx.block().bitcast_i64_to_double(&hit_tagged);
                    let hit_after = ctx.block().label.clone();
                    if !ctx.block().is_terminated() {
                        ctx.block().br(&merge_label);
                    }

                    ctx.current_block = miss_idx;
                    let fallback_delete = Expr::Delete(fallback.clone());
                    let miss = lower_expr(ctx, &fallback_delete)?;
                    let miss_after = ctx.block().label.clone();
                    if !ctx.block().is_terminated() {
                        ctx.block().br(&merge_label);
                    }

                    ctx.current_block = merge_idx;
                    Ok(ctx
                        .block()
                        .phi(DOUBLE, &[(&hit, &hit_after), (&miss, &miss_after)]))
                }
                // #1344: `delete process.env.X` must unset the real OS
                // environment, not just the cached env dict — reads lower to
                // `EnvGet` → `js_getenv_value` → `std::env::var`, so a generic
                // object-field delete would leave the var still readable.
                // `process.env.X` / `process.env[expr]` lower to
                // `EnvGet` / `EnvGetDynamic`, so the delete operand is one of
                // those. Route to `js_removeenv(key)` and yield `true` (delete
                // of a configurable own property always succeeds in Node).
                Expr::EnvGet(name) => {
                    let key_idx = ctx.strings.intern(name);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let blk = ctx.block();
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_handle = unbox_to_i64(blk, &key_box);
                    blk.call_void("js_removeenv", &[(I64, &key_handle)]);
                    Ok(blk.bitcast_i64_to_double(crate::nanbox::TAG_TRUE_I64))
                }
                Expr::EnvGetDynamic(name_expr) => {
                    let key_box = lower_expr(ctx, name_expr)?;
                    let blk = ctx.block();
                    let key_handle = unbox_str_handle(blk, &key_box);
                    blk.call_void("js_removeenv", &[(I64, &key_handle)]);
                    Ok(blk.bitcast_i64_to_double(crate::nanbox::TAG_TRUE_I64))
                }
                Expr::PropertyGet { object, property } => {
                    let obj_box = lower_expr(ctx, object)?;
                    // `delete null.x` / `delete undefined.x` → TypeError. The
                    // `delete` algorithm calls `ToObject(GetBase)` on a property
                    // reference, which throws for a nullish base.
                    ctx.block()
                        .call(DOUBLE, "js_require_object_coercible", &[(DOUBLE, &obj_box)]);
                    let key_idx = ctx.strings.intern(property);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let strict = if ctx.is_strict_fn { "1" } else { "0" };
                    let blk = ctx.block();
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_handle = unbox_to_i64(blk, &key_box);
                    // Pass the RAW receiver: `delete (primitive).field` must no-op
                    // to `true`, not unbox a non-object as a garbage ObjectHeader*.
                    let i32_v = blk.call(
                        I32,
                        "js_object_delete_field_value",
                        &[(DOUBLE, &obj_box), (I64, &key_handle)],
                    );
                    Ok(blk.call(DOUBLE, "js_delete_result", &[(I32, &i32_v), (I32, strict)]))
                }
                Expr::IndexGet { object, index } if is_string_expr(ctx, index) => {
                    let obj_box = lower_expr(ctx, object)?;
                    let key_box = lower_expr(ctx, index)?;
                    // `delete null[k]` / `delete undefined[k]` → TypeError, after
                    // the key expression is evaluated (spec
                    // EvaluatePropertyAccessWithExpressionKey: RequireObjectCoercible
                    // runs after ToPropertyKey's operand is evaluated).
                    ctx.block()
                        .call(DOUBLE, "js_require_object_coercible", &[(DOUBLE, &obj_box)]);
                    let strict = if ctx.is_strict_fn { "1" } else { "0" };
                    let blk = ctx.block();
                    // SSO-safe key unbox — `js_object_delete_field`
                    // dereferences the key as `*StringHeader`. #214 class.
                    let key_handle = unbox_str_handle(blk, &key_box);
                    // Raw receiver: primitive `delete prim[k]` no-ops to `true`.
                    let i32_v = blk.call(
                        I32,
                        "js_object_delete_field_value",
                        &[(DOUBLE, &obj_box), (I64, &key_handle)],
                    );
                    Ok(blk.call(DOUBLE, "js_delete_result", &[(I32, &i32_v), (I32, strict)]))
                }
                // delete obj[expr] — route dynamic keys through the runtime so
                // string-valued locals (for example `delete fn[name]`) still
                // use the ordinary property-delete path instead of being
                // misread as numeric array indexes.
                Expr::IndexGet { object, index } => {
                    let obj_box = lower_expr(ctx, object)?;
                    let idx_box = lower_expr(ctx, index)?;
                    ctx.block()
                        .call(DOUBLE, "js_require_object_coercible", &[(DOUBLE, &obj_box)]);
                    let strict = if ctx.is_strict_fn { "1" } else { "0" };
                    let blk = ctx.block();
                    // Raw receiver: primitive `delete prim[expr]` no-ops to `true`.
                    let i32_v = blk.call(
                        I32,
                        "js_object_delete_dynamic_value",
                        &[(DOUBLE, &obj_box), (DOUBLE, &idx_box)],
                    );
                    Ok(blk.call(DOUBLE, "js_delete_result", &[(I32, &i32_v), (I32, strict)]))
                }
                _ => {
                    let _ = lower_expr(ctx, operand)?;
                    Ok(ctx
                        .block()
                        .bitcast_i64_to_double(crate::nanbox::TAG_TRUE_I64))
                }
            }
        }

        // -------- Sequence (comma operator) --------
        // Evaluate every sub-expression in order, return the last.
        Expr::Sequence(exprs) => {
            let mut last = double_literal(0.0);
            for e in exprs {
                last = lower_expr(ctx, e)?;
            }
            Ok(last)
        }

        // -------- Array.from(iterable) — stub returns the iterable as-is --------
        // Array.from(iterable) — clone via js_array_clone which
        // handles arrays, Sets (→ js_set_to_array), Maps (→ entries).
        Expr::ArrayFrom(iter) => {
            // #2773: `js_array_from_value` throws TypeError for null/undefined
            // sources (and keeps number/boolean/symbol -> []) before delegating
            // to the existing `js_array_clone` materialization. Pass the raw
            // NaN-boxed value (NOT unboxed) so the tag bits survive.
            let iter_box = lower_expr(ctx, iter)?;
            let blk = ctx.block();
            let result = blk.call(I64, "js_array_from_value", &[(DOUBLE, &iter_box)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }

        Expr::ArrayFromArrayLikeHoley(iter) => {
            let iter_box = lower_expr(ctx, iter)?;
            let blk = ctx.block();
            let result = blk.call(
                I64,
                "js_array_from_arraylike_holey_value",
                &[(DOUBLE, &iter_box)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // `Iterator.from(x)` (#2874) — wrap any iterable/iterator in a TC39
        // iterator-helper object so the lazy helper methods (map/filter/take/
        // drop/flatMap/reduce/toArray/...) dispatch at runtime against
        // `ITERATOR_HELPER_CLASS_ID`. `js_iterator_from` takes and returns a
        // NaN-boxed f64, so pass the boxed value straight through and return
        // the boxed result directly.
        Expr::IteratorFrom(iter) => {
            let iter_box = lower_expr(ctx, iter)?;
            let blk = ctx.block();
            Ok(blk.call(DOUBLE, "js_iterator_from", &[(DOUBLE, &iter_box)]))
        }

        // Tagged-template strings literal — build cooked array, build raw
        // array, then fetch/init the frozen per-call-site template object.
        // Same emit shape as the generic `Expr::Array` lowering but with
        // the template-object initialization sandwiched in.
        Expr::TaggedTemplateStrings {
            site_id,
            cooked,
            raw,
        } => {
            // Materialize cooked array — go through lower_array_literal so
            // SSO + GC + length-init logic stays in one place.
            let cooked_box = lower_array_literal(ctx, cooked)?;
            // Materialize raw array — same path, but all elements are
            // String literals (built at HIR lowering from each quasi's
            // `.raw` text), so build a Vec<Expr::String> on the fly.
            let raw_exprs: Vec<Expr> = raw.iter().map(|s| Expr::String(s.clone())).collect();
            let raw_box = lower_array_literal(ctx, &raw_exprs)?;
            let blk = ctx.block();
            let cooked_handle = unbox_to_i64(blk, &cooked_box);
            let raw_handle = unbox_to_i64(blk, &raw_box);
            let site_id = i64_literal(*site_id);
            let registered = blk.call(
                I64,
                "js_tagged_template_get_or_init",
                &[(I64, &site_id), (I64, &cooked_handle), (I64, &raw_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &registered))
        }

        // `strings.raw` — look up the registered raw-strings array for a
        // tagged-template receiver. Non-tagged receivers naturally miss
        // the side table and the helper returns 0 which we surface as
        // TAG_UNDEFINED (matches the JS semantics `[].raw === undefined`).
        Expr::TemplateRaw(obj) => {
            let obj_box = lower_expr(ctx, obj)?;
            let blk = ctx.block();
            let obj_handle = unbox_to_i64(blk, &obj_box);
            let raw_handle = blk.call(I64, "js_template_raw", &[(I64, &obj_handle)]);
            // If the side-table missed (raw_handle == 0), return undefined;
            // otherwise NaN-box as a pointer.
            let is_zero = blk.icmp_eq(I64, &raw_handle, "0");
            let ptr_boxed = nanbox_pointer_inline(ctx.block(), &raw_handle);
            let ptr_bits = ctx.block().bitcast_double_to_i64(&ptr_boxed);
            let selected = ctx.block().select(
                I1,
                &is_zero,
                I64,
                crate::nanbox::TAG_UNDEFINED_I64,
                &ptr_bits,
            );
            Ok(ctx.block().bitcast_i64_to_double(&selected))
        }
        Expr::ArrayFromMapped {
            iterable,
            map_fn,
            this_arg,
        } => {
            // #2773: `js_array_from_mapped` throws for nullish sources, validates
            // mapFn callability, calls mapFn(value, index) and binds the optional
            // thisArg. All three args are passed raw NaN-boxed (DOUBLE).
            let iter_box = lower_expr(ctx, iterable)?;
            let cb_box = lower_expr(ctx, map_fn)?;
            let this_box = match this_arg {
                Some(t) => lower_expr(ctx, t)?,
                None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            };
            let blk = ctx.block();
            let mapped = blk.call(
                I64,
                "js_array_from_mapped",
                &[(DOUBLE, &iter_box), (DOUBLE, &cb_box), (DOUBLE, &this_box)],
            );
            Ok(nanbox_pointer_inline(blk, &mapped))
        }
        Expr::Uint8ArrayFrom(iter) => {
            // #2774: materialize the source into a real Uint8Array (kind 1) so
            // `Uint8Array.from(...)` / `Uint8Array.of(...)` produce typed arrays
            // (with Uint8 truncation), not plain Arrays. Source nullish-throwing
            // + materialization is reused from `js_array_from_value`.
            let iter_box = lower_expr(ctx, iter)?;
            let blk = ctx.block();
            let arr = blk.call(I64, "js_array_from_value", &[(DOUBLE, &iter_box)]);
            // Perry represents `Uint8Array` as a buffer-backed object
            // (`BufferHeader`, see buffer/from.rs), NOT the generic
            // `TypedArrayHeader` kind-1 produced by
            // `js_typed_array_new_from_array`. `new Uint8Array([...])` already
            // builds the buffer form; routing `Uint8Array.of/from` through the
            // same `js_uint8array_from_array` keeps the representation
            // consistent so element reads (`u[i]`) go through the registered
            // buffer path instead of mis-reading a TypedArrayHeader as a plain
            // array (issue #871: of/from produced garbage bytes).
            let ta = blk.call(I64, "js_uint8array_from_array", &[(I64, &arr)]);
            Ok(nanbox_pointer_inline(blk, &ta))
        }

        // -------- Object.values / Object.entries --------
        Expr::ObjectValues(obj) => {
            let obj_box = lower_expr(ctx, obj)?;
            let blk = ctx.block();
            // Tagged value so a string/primitive receiver is handled safely.
            let arr_handle = blk.call(I64, "js_object_values_value", &[(DOUBLE, &obj_box)]);
            Ok(nanbox_pointer_inline(blk, &arr_handle))
        }
        Expr::ObjectEntries(obj) => {
            let obj_box = lower_expr(ctx, obj)?;
            let blk = ctx.block();
            let arr_handle = blk.call(I64, "js_object_entries_value", &[(DOUBLE, &obj_box)]);
            Ok(nanbox_pointer_inline(blk, &arr_handle))
        }

        // -------- path.join(a, b) -> string --------
        // The HIR variant is binary; multi-arg path.join lowers to
        // chained PathJoin in the HIR.
        Expr::PathJoin(a, b) => {
            let a_box = lower_expr(ctx, a)?;
            let b_box = lower_expr(ctx, b)?;
            let blk = ctx.block();
            let a_handle = unbox_to_i64(blk, &a_box);
            let b_handle = unbox_to_i64(blk, &b_box);
            let result = blk.call(I64, "js_path_join", &[(I64, &a_handle), (I64, &b_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }

        // -------- path.win32.join(a, b) -> string (issue #810) --------
        // Windows-style join with `\` separator, regardless of host
        // platform. Multi-arg path.win32.join lowers to chained
        // PathWin32Join in the HIR.
        Expr::PathWin32Join(a, b) => {
            let a_box = lower_expr(ctx, a)?;
            let b_box = lower_expr(ctx, b)?;
            let blk = ctx.block();
            let a_handle = unbox_to_i64(blk, &a_box);
            let b_handle = unbox_to_i64(blk, &b_box);
            let result = blk.call(
                I64,
                "js_path_win32_join",
                &[(I64, &a_handle), (I64, &b_handle)],
            );
            Ok(nanbox_string_inline(blk, &result))
        }

        // -------- path.win32.<method>(...) (issue #1162) --------
        // One arm covers every win32 sub-namespace method other than
        // `.join` (above), `.sep`, and `.delimiter` (string literals
        // folded at lowering time). Dispatch on `method` to the matching
        // js_path_win32_* runtime function.
        Expr::PathWin32 { method, args } => {
            use perry_hir::PathWin32Method;
            // Lower all args up front into NaN-boxed JSValue locals.
            let lowered: Vec<_> = args
                .iter()
                .map(|a| lower_expr(ctx, a))
                .collect::<Result<Vec<_>, _>>()?;
            match method {
                PathWin32Method::ToNamespacedPath => {
                    let blk = ctx.block();
                    Ok(blk.call(
                        DOUBLE,
                        "js_path_win32_to_namespaced_path_value",
                        &[(DOUBLE, &lowered[0])],
                    ))
                }
                PathWin32Method::Dirname
                | PathWin32Method::Basename
                | PathWin32Method::Extname
                | PathWin32Method::Normalize
                | PathWin32Method::Resolve => {
                    let fn_name = match method {
                        PathWin32Method::Dirname => "js_path_win32_dirname",
                        PathWin32Method::Basename => "js_path_win32_basename",
                        PathWin32Method::Extname => "js_path_win32_extname",
                        PathWin32Method::Normalize => "js_path_win32_normalize",
                        PathWin32Method::Resolve => "js_path_win32_resolve",
                        _ => unreachable!(),
                    };
                    let blk = ctx.block();
                    let h = unbox_to_i64(blk, &lowered[0]);
                    let result = blk.call(I64, fn_name, &[(I64, &h)]);
                    Ok(nanbox_string_inline(blk, &result))
                }
                PathWin32Method::Relative => {
                    // #2995: validate both operands are strings (throwing
                    // ERR_INVALID_ARG_TYPE on a non-string) before computing
                    // the relative path. Pass the NaN-boxed doubles so the
                    // runtime can inspect their type.
                    let blk = ctx.block();
                    let result = blk.call(
                        I64,
                        "js_path_win32_relative_checked",
                        &[(DOUBLE, &lowered[0]), (DOUBLE, &lowered[1])],
                    );
                    Ok(nanbox_string_inline(blk, &result))
                }
                PathWin32Method::BasenameExt | PathWin32Method::ResolveJoin => {
                    let fn_name = match method {
                        PathWin32Method::BasenameExt => "js_path_win32_basename_ext",
                        PathWin32Method::ResolveJoin => "js_path_win32_resolve_join",
                        _ => unreachable!(),
                    };
                    let blk = ctx.block();
                    let a = unbox_to_i64(blk, &lowered[0]);
                    let b = unbox_to_i64(blk, &lowered[1]);
                    let result = blk.call(I64, fn_name, &[(I64, &a), (I64, &b)]);
                    Ok(nanbox_string_inline(blk, &result))
                }
                PathWin32Method::IsAbsolute => {
                    let blk = ctx.block();
                    let h = unbox_to_i64(blk, &lowered[0]);
                    let i32_v = blk.call(I32, "js_path_win32_is_absolute", &[(I64, &h)]);
                    Ok(i32_bool_to_nanbox(blk, &i32_v))
                }
                PathWin32Method::MatchesGlob => {
                    let blk = ctx.block();
                    let p = unbox_to_i64(blk, &lowered[0]);
                    let pat = unbox_to_i64(blk, &lowered[1]);
                    let i32_v =
                        blk.call(I32, "js_path_win32_matches_glob", &[(I64, &p), (I64, &pat)]);
                    Ok(i32_bool_to_nanbox(blk, &i32_v))
                }
                PathWin32Method::Parse => {
                    let blk = ctx.block();
                    let h = unbox_to_i64(blk, &lowered[0]);
                    let result = blk.call(I64, "js_path_win32_parse", &[(I64, &h)]);
                    Ok(nanbox_pointer_inline(blk, &result))
                }
                PathWin32Method::Format => {
                    // js_path_win32_format takes a NaN-boxed double (object handle).
                    let obj_box = lowered.into_iter().next().unwrap();
                    let blk = ctx.block();
                    let result = blk.call(I64, "js_path_win32_format", &[(DOUBLE, &obj_box)]);
                    Ok(nanbox_string_inline(blk, &result))
                }
            }
        }

        // -------- queueMicrotask(fn) stub --------
        Expr::QueueMicrotask(cb) => {
            let cb_box = lower_expr(ctx, cb)?;
            let blk = ctx.block();
            let cb_handle = blk.call(
                I64,
                "js_timer_validate_callback",
                &[(DOUBLE, &cb_box), (I32, "-1")],
            );
            blk.call_void("js_queue_microtask", &[(I64, &cb_handle)]);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // -------- process.nextTick(fn, ...args) --------
        // Trailing args are forwarded to the callback when the tick fires
        // (#1351). Pack them into a stack buffer of doubles and hand off to
        // the varargs runtime entry; the no-args form goes through the
        // simpler `js_queue_next_tick` to avoid the alloca cost.
        Expr::ProcessNextTick { callback, args } => {
            let cb_box = lower_expr(ctx, callback)?;
            if args.is_empty() {
                let blk = ctx.block();
                // #3046: validate the callback (non-callable → Node's
                // `ERR_INVALID_ARG_TYPE` "callback" message) before queueing.
                // `js_timer_validate_callback` always reports the "callback"
                // argument name and returns the closure handle; idx 3 selects
                // its generic-callback wording branch.
                let cb_handle = blk.call(
                    I64,
                    "js_timer_validate_callback",
                    &[(DOUBLE, &cb_box), (I32, "3")],
                );
                blk.call_void("js_queue_next_tick", &[(I64, &cb_handle)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let n = args.len();
            let buf = ctx.func.alloca_entry_array(DOUBLE, n);
            for (i, a) in args.iter().enumerate() {
                let v = lower_expr(ctx, a)?;
                let blk = ctx.block();
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
                blk.store(DOUBLE, &v, &slot);
            }
            let ptr_reg = ctx.block().next_reg();
            ctx.block().emit_raw(format!(
                "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                ptr_reg, n, buf
            ));
            let blk = ctx.block();
            // #3046: same callback validation on the trailing-args path.
            let cb_handle = blk.call(
                I64,
                "js_timer_validate_callback",
                &[(DOUBLE, &cb_box), (I32, "3")],
            );
            blk.call_void(
                "js_queue_next_tick_args",
                &[(I64, &cb_handle), (PTR, &ptr_reg), (I32, &n.to_string())],
            );
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // -------- RegExpTest --------
        // regex.test(str) -> boolean. Real call to js_regexp_test.
        // Receiver is a NaN-tagged i64 RegExpHeader pointer; arg is
        // a NaN-tagged string. Both must be unboxed before the call.
        Expr::RegExpTest { regex, string } => {
            let regex_box = lower_expr(ctx, regex)?;
            let str_box = lower_expr(ctx, string)?;
            let blk = ctx.block();
            let regex_handle = unbox_to_i64(blk, &regex_box);
            // Per spec `RegExp.prototype.test` does `ToString(argument)`, so a
            // String wrapper (`re.test(new String("x"))`), a number
            // (`re.test(123)`), or an object with a custom `toString` must be
            // coerced — and a throwing `toString`/`valueOf` must propagate.
            // `js_get_string_pointer_unified` only unwraps real strings, so use
            // the coercing ToString that dispatches `toString` on objects.
            let str_handle = blk.call(I64, "js_jsvalue_to_string_coerce", &[(DOUBLE, &str_box)]);
            let i32_v = blk.call(
                I32,
                "js_regexp_test",
                &[(I64, &regex_handle), (I64, &str_handle)],
            );
            Ok(i32_bool_to_nanbox(blk, &i32_v))
        }
        Expr::RegExpExec { regex, string } => {
            // Returns ArrayHeader* or null. For a null (0) result we must
            // produce TAG_NULL so `re.exec(s) !== null` loops terminate
            // correctly — just NaN-boxing 0 with POINTER_TAG produces a
            // non-null pointer value that compares unequal to null, causing
            // infinite loops + segfaults when callers IndexGet on the result.
            let regex_box = lower_expr(ctx, regex)?;
            let str_box = lower_expr(ctx, string)?;
            let blk = ctx.block();
            let regex_handle = unbox_to_i64(blk, &regex_box);
            // `RegExp.prototype.exec` does `ToString(argument)` — coerce String
            // wrappers / numbers / objects (and propagate a throwing toString)
            // rather than only unwrapping real strings (see RegExpTest above).
            let str_handle = blk.call(I64, "js_jsvalue_to_string_coerce", &[(DOUBLE, &str_box)]);
            let result = blk.call(
                I64,
                "js_regexp_exec",
                &[(I64, &regex_handle), (I64, &str_handle)],
            );
            // Branch on result == 0 → TAG_NULL; else NaN-box as pointer.
            let is_null = blk.icmp_eq(I64, &result, "0");
            let ptr_boxed = nanbox_pointer_inline(ctx.block(), &result);
            let ptr_bits = ctx.block().bitcast_double_to_i64(&ptr_boxed);
            let selected =
                ctx.block()
                    .select(I1, &is_null, I64, crate::nanbox::TAG_NULL_I64, &ptr_bits);
            Ok(ctx.block().bitcast_i64_to_double(&selected))
        }

        // -------- GlobalGet stub --------
        // Most uses of GlobalGet are inside `PropertyGet { GlobalGet, ... }`
        // which is handled separately. Bare GlobalGet (e.g. passing
        // `console` as a value) returns a sentinel.
        Expr::GlobalGet(_) => Ok(double_literal(0.0)),

        // -------- path.dirname / path.relative --------
        Expr::PathDirname(p) => {
            let p_box = lower_expr(ctx, p)?;
            let blk = ctx.block();
            let p_handle = unbox_to_i64(blk, &p_box);
            let result = blk.call(I64, "js_path_dirname", &[(I64, &p_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        Expr::PathRelative(from, to) => {
            // #2995: validate both operands are strings before computing the
            // relative path. The checked entry point inspects the NaN-boxed
            // doubles and throws ERR_INVALID_ARG_TYPE for non-strings.
            let f_box = lower_expr(ctx, from)?;
            let t_box = lower_expr(ctx, to)?;
            let blk = ctx.block();
            let result = blk.call(
                I64,
                "js_path_relative_checked",
                &[(DOUBLE, &f_box), (DOUBLE, &t_box)],
            );
            Ok(nanbox_string_inline(blk, &result))
        }

        // -------- arr.includes(value) -> boolean --------
        Expr::ArrayIncludes {
            array,
            value,
            from_index,
        } => {
            let arr_box = lower_expr(ctx, array)?;
            let v = lower_expr(ctx, value)?;
            // #2804: optional fromIndex. has_from=1 + lowered index when
            // present; otherwise has_from=0 with a placeholder DOUBLE (`v`).
            let (from_box, has_from) = match from_index {
                Some(fi) => (lower_expr(ctx, fi)?, "1"),
                None => (v.clone(), "0"),
            };
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            // Use `js_array_includes_jsvalue` which does deep-value
            // equality (string content, not pointer identity). The
            // `*_f64` variant compares raw f64 bits which fails for
            // strings created at different sites.
            let i32_v = blk.call(
                I32,
                "js_array_includes_jsvalue",
                &[
                    (I64, &arr_handle),
                    (DOUBLE, &v),
                    (DOUBLE, &from_box),
                    (I32, has_from),
                ],
            );
            // Convert i32 boolean to NaN-tagged TAG_TRUE/FALSE so
            // console.log prints "true"/"false".
            let bit = blk.icmp_ne(I32, &i32_v, "0");
            let tagged = blk.select(
                crate::types::I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        // -------- arr.splice(start, deleteCount?, ...items) --------
        // Real call to js_array_splice. The runtime returns the
        // deleted elements; the modified array is written to an
        // out-parameter pointer.
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => {
            let arr_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
            let start_d = lower_expr(ctx, start)?;
            let count_d = if let Some(d) = delete_count {
                lower_expr(ctx, d)?
            } else {
                "2147483647.0".to_string()
            };

            // Evaluate splice-insert items and collect their f64 values.
            let mut item_vals: Vec<String> = Vec::new();
            for it in items {
                item_vals.push(lower_expr(ctx, it)?);
            }

            let blk = ctx.block();
            // Scratch out-parameter slot — used only in this block to
            // receive the modified-array handle from js_array_splice.
            let out_slot = blk.alloca(I64);
            blk.store(I64, "0", &out_slot);
            let arr_handle = unbox_to_i64(blk, &arr_box);
            // ToIntegerOrInfinity via the clamping helper: `fptosi` on
            // ±Infinity/NaN is LLVM poison — `splice(Infinity, 3)` deleted
            // from index 0 (test262 splice/S15.4.4.12_A2.1_T3).
            let start_i32 = blk.call(I32, "js_array_splice_delete_count", &[(DOUBLE, &start_d)]);
            let count_i32 = blk.call(I32, "js_array_splice_delete_count", &[(DOUBLE, &count_d)]);

            let (items_ptr, items_count_str) = if item_vals.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                // Allocate a stack buffer of [N x double] for the
                // items, store each value, and pass the base pointer.
                let n = item_vals.len();
                let items_count_str = format!("{}", n);
                let buf_reg = blk.next_reg();
                blk.emit_raw(format!("{} = alloca [{} x double]", buf_reg, n));
                for (i, val) in item_vals.iter().enumerate() {
                    let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, val, &slot);
                }
                (buf_reg, items_count_str)
            };

            // Note: js_array_splice's return value is the DELETED
            // array; the modified-in-place arr is written to *out_arr.
            let deleted_handle = blk.call(
                I64,
                "js_array_splice",
                &[
                    (I64, &arr_handle),
                    (I32, &start_i32),
                    (I32, &count_i32),
                    (PTR, &items_ptr),
                    (I32, &items_count_str),
                    (PTR, &out_slot),
                ],
            );
            // Read the modified array from the out slot and write it
            // back to the source local.
            let modified_handle = ctx.block().load(I64, &out_slot);
            let modified_box = nanbox_pointer_inline(ctx.block(), &modified_handle);
            if let Some(slot) = ctx.locals.get(array_id).cloned() {
                ctx.block().store(DOUBLE, &modified_box, &slot);
            } else if let Some(global_name) = ctx.module_globals.get(array_id).cloned() {
                let g_ref = format!("@{}", global_name);
                // GC_STORE_AUDIT(ROOT): module global array slot is a registered mutable GC root.
                emit_root_nanbox_store_on_block(ctx.block(), &modified_box, &g_ref);
            }
            // Return the deleted array (NaN-boxed) as the splice
            // expression's value.
            Ok(nanbox_pointer_inline(ctx.block(), &deleted_handle))
        }

        // -------- ObjectFromEntries (passes through to runtime) --------
        Expr::ObjectFromEntries(arr) => {
            let v = lower_expr(ctx, arr)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_object_from_entries", &[(DOUBLE, &v)]))
        }

        // -------- Object.groupBy(items, keyFn) --------
        // Routes through `js_object_group_by(items_value, callback)`.
        // Both args are NaN-boxed f64; the runtime validates iterability and
        // callback callability (TypeError on failure) per Node semantics.
        Expr::ObjectGroupBy { items, key_fn } => {
            let items_v = lower_expr(ctx, items)?;
            let cb_v = lower_expr(ctx, key_fn)?;
            let blk = ctx.block();
            Ok(blk.call(
                DOUBLE,
                "js_object_group_by",
                &[(DOUBLE, &items_v), (DOUBLE, &cb_v)],
            ))
        }

        // -------- Map.groupBy(items, keyFn) --------
        // Routes through `js_map_group_by(items_value, callback)` — returns a
        // Map keyed by callback results without string coercion.
        Expr::MapGroupBy { items, key_fn } => {
            let items_v = lower_expr(ctx, items)?;
            let cb_v = lower_expr(ctx, key_fn)?;
            let blk = ctx.block();
            Ok(blk.call(
                DOUBLE,
                "js_map_group_by",
                &[(DOUBLE, &items_v), (DOUBLE, &cb_v)],
            ))
        }

        // -------- string.match(regex) --------
        Expr::StringMatch { string, regex } => {
            let s_box = lower_expr(ctx, string)?;
            let r_box = lower_expr(ctx, regex)?;
            let blk = ctx.block();
            // SSO-safe string-receiver unbox: `js_string_match` reads
            // `byte_len` and the UTF-8 bytes from the StringHeader, which
            // segfaults on SSO inline bits. SIGSEGV repro:
            // `JSON.parse('"abc"').match(/b/)`. #214 SSO bug class.
            let s_handle = unbox_str_handle(blk, &s_box);
            let r_handle = unbox_to_i64(blk, &r_box);
            let result = blk.call(
                I64,
                "js_string_match",
                &[(I64, &s_handle), (I64, &r_handle)],
            );
            // #4858: js_string_match returns null (0) on no-match. NaN-boxing
            // 0 with POINTER_TAG yields a value that is neither `null` nor a
            // valid heap pointer — `s.match(/x/g) === null` was false and
            // consumers that deref the result (JSON.stringify, .map) crashed.
            // Branchless null → TAG_NULL select, same as RegExpExec above.
            let is_null = blk.icmp_eq(I64, &result, "0");
            let ptr_boxed = nanbox_pointer_inline(ctx.block(), &result);
            let ptr_bits = ctx.block().bitcast_double_to_i64(&ptr_boxed);
            let selected =
                ctx.block()
                    .select(I1, &is_null, I64, crate::nanbox::TAG_NULL_I64, &ptr_bits);
            Ok(ctx.block().bitcast_i64_to_double(&selected))
        }

        // -------- string.matchAll(pattern) --------
        // Returns a RegExp String Iterator object. SSO-safe receiver unbox via
        // `unbox_str_handle` for the same reason as `StringMatch`; pass the raw
        // pattern value so runtime can validate RegExp globals or create a
        // global RegExp for string/non-RegExp patterns.
        Expr::StringMatchAll { string, regex } => {
            let s_box = lower_expr(ctx, string)?;
            let r_box = lower_expr(ctx, regex)?;
            let blk = ctx.block();
            let s_handle = unbox_str_handle(blk, &s_box);
            let result = blk.call(
                I64,
                "js_string_match_all_value",
                &[(I64, &s_handle), (DOUBLE, &r_box)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- obj.field++ / obj.field-- (PropertyUpdate) --------
        // Lowered as: load → fadd/fsub 1.0 → store. Same as the
        // Update variant but for a property instead of a local.
        Expr::PropertyUpdate {
            object,
            property,
            op,
            prefix,
        } => {
            // Scalar replacement fast path: load → fadd/fsub 1.0 → store
            // on the field's alloca, no heap traffic.
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(slot) = ctx
                    .scalar_replaced
                    .get(id)
                    .and_then(|fs| fs.get(property.as_str()))
                    .cloned()
                {
                    let blk = ctx.block();
                    let old = blk.load(DOUBLE, &slot);
                    let old_num = blk.call(DOUBLE, "js_number_coerce", &[(DOUBLE, &old)]);
                    let new = match op {
                        BinaryOp::Sub => blk.fsub(&old_num, "1.0"),
                        _ => blk.fadd(&old_num, "1.0"),
                    };
                    blk.store(DOUBLE, &new, &slot);
                    return Ok(if *prefix { new } else { old_num });
                }
            }
            if let Expr::This = object.as_ref() {
                if let Some(slot) = ctx
                    .scalar_ctor_target
                    .last()
                    .and_then(|tid| ctx.scalar_replaced.get(tid))
                    .and_then(|fs| fs.get(property.as_str()))
                    .cloned()
                {
                    let blk = ctx.block();
                    let old = blk.load(DOUBLE, &slot);
                    let old_num = blk.call(DOUBLE, "js_number_coerce", &[(DOUBLE, &old)]);
                    let new = match op {
                        BinaryOp::Sub => blk.fsub(&old_num, "1.0"),
                        _ => blk.fadd(&old_num, "1.0"),
                    };
                    blk.store(DOUBLE, &new, &slot);
                    return Ok(if *prefix { new } else { old_num });
                }
            }
            let obj_box = lower_expr(ctx, object)?;
            let key_idx = ctx.strings.intern(property);
            let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let blk = ctx.block();
            let obj_bits = blk.bitcast_double_to_i64(&obj_box);
            let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
            let key_box = blk.load(DOUBLE, &key_handle_global);
            let key_bits = blk.bitcast_double_to_i64(&key_box);
            let key_handle = blk.and(I64, &key_bits, POINTER_MASK_I64);
            let old = blk.call(
                DOUBLE,
                "js_object_get_field_by_name_f64",
                &[(I64, &obj_handle), (I64, &key_handle)],
            );
            // ToNumeric + Type(old)::add/sub(old, unit): a BigInt field stays a
            // BigInt (`var x = {y:0n}; ++x.y === 1n`), not the Number `1`. Mirrors
            // the identifier `Expr::Update` path. #4918 prefix/postfix bigint.
            let old_num = blk.call(DOUBLE, "js_to_numeric", &[(DOUBLE, &old)]);
            let step_arg = match op {
                BinaryOp::Sub => "0",
                _ => "1",
            };
            let new = blk.call(
                DOUBLE,
                "js_numeric_step",
                &[(DOUBLE, &old_num), (I32, step_arg)],
            );
            blk.call_void(
                "js_object_set_field_by_name",
                &[(I64, &obj_handle), (I64, &key_handle), (DOUBLE, &new)],
            );
            Ok(if *prefix { new } else { old_num })
        }

        // -------- arr[idx]++ / arr[idx]-- / ++arr[idx] / --arr[idx] --------
        //
        // Issue #957: lodash's `countBy` uses `++result[key]` which previously
        // bailed `expression IndexUpdate not yet supported` and stubbed the
        // entire module, leaving `import _ from "lodash"` resolving to
        // undefined. Lower as a tag-aware read+modify+write through the
        // `js_dyn_index_get` / `js_dyn_index_set` runtime helpers — they
        // dispatch by gc_type at runtime, so the same emission works for
        // arrays, plain objects, and TypedArrays without static type
        // knowledge. `object` and `index` lower once into SSA registers so
        // side effects are not re-evaluated.
        Expr::IndexUpdate {
            object,
            index,
            op,
            prefix,
        } => {
            let obj_box = lower_expr(ctx, object)?;
            let idx_box = lower_expr(ctx, index)?;
            let blk = ctx.block();
            let old = blk.call(
                DOUBLE,
                "js_dyn_index_get",
                &[(DOUBLE, &obj_box), (DOUBLE, &idx_box)],
            );
            // ToNumeric + numeric step so a BigInt element stays BigInt
            // (`var x = [0n]; ++x[0] === 1n`). Mirrors the identifier Update +
            // PropertyUpdate paths. #4918 prefix/postfix bigint.
            let old_num = blk.call(DOUBLE, "js_to_numeric", &[(DOUBLE, &old)]);
            let step_arg = match op {
                BinaryOp::Sub => "0",
                _ => "1",
            };
            let new = blk.call(
                DOUBLE,
                "js_numeric_step",
                &[(DOUBLE, &old_num), (I32, step_arg)],
            );
            blk.call(
                DOUBLE,
                "js_dyn_index_set",
                &[(DOUBLE, &obj_box), (DOUBLE, &idx_box), (DOUBLE, &new)],
            );
            Ok(if *prefix { new } else { old_num })
        }

        // -------- path.basename --------
        Expr::PathBasename(p) => {
            let p_box = lower_expr(ctx, p)?;
            let blk = ctx.block();
            let p_handle = unbox_to_i64(blk, &p_box);
            let result = blk.call(I64, "js_path_basename", &[(I64, &p_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        Expr::PathBasenameExt(p, ext) => {
            // path.basename(path, ext) — strips trailing `ext` suffix.
            // Runtime: js_path_basename_ext(path_ptr, ext_ptr) -> *StringHeader.
            let p_box = lower_expr(ctx, p)?;
            let e_box = lower_expr(ctx, ext)?;
            let blk = ctx.block();
            let p_handle = unbox_to_i64(blk, &p_box);
            let e_handle = unbox_to_i64(blk, &e_box);
            let result = blk.call(
                I64,
                "js_path_basename_ext",
                &[(I64, &p_handle), (I64, &e_handle)],
            );
            Ok(nanbox_string_inline(blk, &result))
        }
        Expr::PathParse(p) => {
            // path.parse(p) -> object with { dir, base, ext, name, root }
            let p_box = lower_expr(ctx, p)?;
            let blk = ctx.block();
            let p_handle = unbox_to_i64(blk, &p_box);
            let result = blk.call(I64, "js_path_parse", &[(I64, &p_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- JSON.parse --------
        // js_json_parse returns JSValue (u64 / i64) not f64.
        // Bitcast from i64 to double to stay in the NaN-boxed f64 ABI.
        Expr::JsonParse(text) => {
            let s_box = lower_expr(ctx, text)?;
            let blk = ctx.block();
            // ECMA-262 JSON.parse step 1: jsonText = ? ToString(text). So
            // `JSON.parse(null)` → "null" → null, `JSON.parse(123)` → "123" →
            // 123, and a Symbol arg throws TypeError. `js_json_text_to_string`
            // is ToString (throwing on symbols) and returns a real heap
            // `*StringHeader` (identity for heap strings, materializes SSO to
            // the heap), so it also fixes the #214 SIGSEGV that a bare
            // `unbox_to_i64` of an SSO short-string caused.
            let s_handle = blk.call(I64, "js_json_text_to_string", &[(DOUBLE, &s_box)]);
            let result_i64 = blk.call(I64, "js_json_parse", &[(I64, &s_handle)]);
            Ok(blk.bitcast_i64_to_double(&result_i64))
        }
        // -------- JSON.rawJSON / JSON.isRawJSON (#2900) --------
        // Both runtime helpers take and return a NaN-boxed f64, so the text /
        // value operand passes straight through.
        Expr::JsonRawJson(text) => {
            let s_box = lower_expr(ctx, text)?;
            let blk = ctx.block();
            Ok(blk.call(DOUBLE, "js_json_raw_json", &[(DOUBLE, &s_box)]))
        }
        Expr::JsonIsRawJson(value) => {
            let v_box = lower_expr(ctx, value)?;
            let blk = ctx.block();
            Ok(blk.call(DOUBLE, "js_json_is_raw_json", &[(DOUBLE, &v_box)]))
        }
        // Issue #179 typed-parse, Step 1b: when `<T>` is
        // `Array<Object{fields}>`, emit a packed-keys rodata constant
        // and route through `js_json_parse_typed_array`. Any other
        // shape (or unresolved Named type) falls through to the
        // generic `js_json_parse`. Runtime semantics identical either
        // way — the typed variant is a pure perf specialization.
        Expr::JsonParseTyped {
            text,
            ty,
            ordered_keys,
        } => {
            let packed = extract_array_of_object_shape(ty, ordered_keys.as_deref());
            let s_box = lower_expr(ctx, text)?;
            let blk = ctx.block();
            // Same SSO-materialization fix as the generic JSON.parse arm above:
            // a raw unbox would pass SSO inline bytes as a StringHeader pointer.
            let s_handle = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &s_box)]);
            let result_i64 = match packed {
                Some((packed_bytes, field_count)) if field_count > 0 => {
                    // Emit a per-call-site rodata constant. The IR
                    // byte-escape format matches what
                    // `add_named_string_constant` produces elsewhere.
                    let idx = ctx.typed_parse_counter;
                    ctx.typed_parse_counter += 1;
                    let gname = format!("perry_typed_parse_keys_{}", idx);
                    let bytes_len = packed_bytes.len();
                    let mut lit = String::with_capacity(bytes_len + 8);
                    lit.push('c');
                    lit.push('"');
                    for &b in &packed_bytes {
                        if (32..127).contains(&b) && b != b'"' && b != b'\\' {
                            lit.push(b as char);
                        } else {
                            lit.push('\\');
                            lit.push_str(&format!("{:02X}", b));
                        }
                    }
                    lit.push('"');
                    ctx.typed_parse_rodata.push(format!(
                        "@{} = private unnamed_addr constant [{} x i8] {}",
                        gname, bytes_len, lit
                    ));
                    // Convert `ptr @global` to i64 so it matches the
                    // runtime fn's ABI (which takes `i64` for the
                    // packed-keys pointer — same convention as other
                    // runtime calls).
                    let blk = ctx.block();
                    let ptr_reg = blk.fresh_reg();
                    blk.emit_raw(format!("{} = ptrtoint ptr @{} to i64", ptr_reg, gname));
                    let len_lit = format!("{}", bytes_len);
                    let fc_lit = format!("{}", field_count);
                    blk.call(
                        I64,
                        "js_json_parse_typed_array",
                        &[
                            (I64, &s_handle),
                            (I64, &ptr_reg),
                            (I32, &len_lit),
                            (I32, &fc_lit),
                        ],
                    )
                }
                _ => {
                    // Fall through to generic parse for unhandled shapes.
                    blk.call(I64, "js_json_parse", &[(I64, &s_handle)])
                }
            };
            let blk = ctx.block();
            Ok(blk.bitcast_i64_to_double(&result_i64))
        }
        Expr::JsonParseReviver { text, reviver } => {
            let s_box = lower_expr(ctx, text)?;
            let r_box = lower_expr(ctx, reviver)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let r_handle = unbox_to_i64(blk, &r_box);
            let result_i64 = blk.call(
                I64,
                "js_json_parse_with_reviver",
                &[(I64, &s_handle), (I64, &r_handle)],
            );
            Ok(blk.bitcast_i64_to_double(&result_i64))
        }
        Expr::JsonParseWithReviver(text, reviver) => {
            let s_box = lower_expr(ctx, text)?;
            let r_box = lower_expr(ctx, reviver)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let r_handle = unbox_to_i64(blk, &r_box);
            let result_i64 = blk.call(
                I64,
                "js_json_parse_with_reviver",
                &[(I64, &s_handle), (I64, &r_handle)],
            );
            Ok(blk.bitcast_i64_to_double(&result_i64))
        }

        // -------- new Date() / new Date(ts) / new Date(year, month, ...) --------
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
