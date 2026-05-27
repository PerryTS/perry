//! PropertyGet — guarded specializations + general catchall.
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
use crate::native_value::{
    BoundsState, BufferAccessMode, LoweredValue, MaterializationReason, NativeRep, SemanticKind,
};
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
    emit_typed_feedback_register_site, emit_v8_export_call, emit_v8_member_method_call,
    emit_write_barrier, emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
    extract_array_of_object_shape, i32_bool_to_nanbox, import_origin_suffix,
    is_global_this_builtin_function_name, is_global_this_builtin_name, is_known_finite,
    lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32,
    lower_index_set_fast, lower_js_args_array, lower_object_literal, lower_stream_super_init,
    lower_url_string_getter, nanbox_bigint_inline, nanbox_pointer_inline,
    nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array, raw_f64_layout_fact,
    try_flat_const_2d_int, try_lower_flat_const_index_get, try_lower_pod_field_get,
    try_match_channel_reduction, try_static_class_name, unbox_str_handle, unbox_to_i64,
    variant_name, ChannelReduction, FlatConstInfo, FnCtx, I18nLowerCtx, TypedFeedbackContract,
    TypedFeedbackKind,
};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::PropertyGet { object, property }
            if matches!(object.as_ref(), Expr::LocalGet(id)
                if ctx.pod_records.get(id).is_some_and(|local| local
                    .layout
                    .fields
                    .iter()
                    .any(|field| field.name == *property))) =>
        {
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(value) = try_lower_pod_field_get(ctx, *id, property)? {
                    return Ok(value);
                }
            }
            unreachable!("POD field guard should imply a lowered field")
        }
        Expr::PropertyGet { object, property }
            if property == "length"
                && matches!(
                    object.as_ref(),
                    Expr::PropertyGet { property: p, .. } if p == "errors"
                ) =>
        {
            let recv_box = lower_expr(ctx, object)?;
            let blk = ctx.block();
            let recv_bits = blk.bitcast_double_to_i64(&recv_box);
            let recv_handle = blk.and(I64, &recv_bits, POINTER_MASK_I64);
            let len_i32 = blk.safe_load_i32_from_ptr(&recv_handle);
            Ok(blk.sitofp(I32, &len_i32, DOUBLE))
        }

        // Phase H err: `agg.errors` — AggregateError.errors field.
        // Routes through js_error_get_errors which pulls the raw
        // ArrayHeader pointer from the ErrorHeader struct. Returns a
        // NaN-boxed pointer so downstream length / index operations
        // see an array.
        Expr::PropertyGet { object, property } if property == "errors" => {
            let recv_box = lower_expr(ctx, object)?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let arr_handle = blk.call(I64, "js_error_get_errors", &[(I64, &recv_handle)]);
            Ok(nanbox_pointer_inline(blk, &arr_handle))
        }

        // Buffer / Uint8Array `.length` — INLINE for locals with a
        // pre-computed `buffer_data_slots` entry. Length lives 8 bytes
        // before the data start (BufferHeader). The slot is populated
        // by `Stmt::Let` for `const x = Buffer.alloc(N)` / `new
        // Uint8Array(N)` whose binding doesn't escape — same locals
        // that get the GEP-based fast path in `Expr::Uint8ArrayGet`.
        // Marked `!invariant.load` so LICM can hoist the read out of
        // tight inner loops like image_conv's FNV-1a hash:
        //
        //   for (let i = 0; i < dst.length; i++)
        //
        // Without this, `dst.length` falls through to the GC-type
        // gate below — Buffer/Uint8Array have no GC header (they're
        // `std::alloc`'d), so `gc_type` reads garbage, `has_length`
        // is false, and every iteration calls `js_value_length_f64`
        // (function call into the side-table registry). v0.5.83's
        // `is_array || is_string` guard intentionally routes Buffers
        // through that slow path for safety, but the buffer-data-slot
        // path proves the receiver shape at compile time.
        Expr::PropertyGet { object, property }
            if property == "length"
                && matches!(object.as_ref(), Expr::LocalGet(id)
                    if ctx.buffer_data_slots.contains_key(id)) =>
        {
            let arr_id = match object.as_ref() {
                Expr::LocalGet(id) => *id,
                _ => unreachable!(),
            };
            let (ptr_slot, _scope) = ctx.buffer_data_slots.get(&arr_id).cloned().unwrap();
            let blk = ctx.block();
            let data_ptr = blk.load(PTR, &ptr_slot);
            let header_ptr = blk.gep(I8, &data_ptr, &[(I32, "-8")]);
            let len_i32 = blk.load_invariant(I32, &header_ptr);
            let lowered = LoweredValue::buffer_len(len_i32);
            ctx.record_lowered_value(
                "Buffer.length",
                Some(arr_id),
                "Buffer.length.native_buffer_len",
                &lowered,
                None,
                None,
                None,
                false,
                false,
                Vec::new(),
            );
            return Ok(crate::native_value::materialize_js_value(
                ctx,
                lowered,
                MaterializationReason::FunctionAbi,
            ));
        }

        // `arr.length` / `str.length` — INLINE. Both ArrayHeader and
        // StringHeader start with `length: u32` (`crates/perry-runtime/src
        // /array.rs` and `string.rs`). Same pattern: unbox pointer, load
        // u32 from offset 0, sitofp to double.
        // `.length` — INLINE for array, string, and interface-typed
        // receivers. Named types (interfaces, class instances) often
        // wrap strings or arrays at runtime, where length is at offset 0.
        Expr::PropertyGet { object, property }
            if property == "length"
                && (is_array_expr(ctx, object)
                    || is_string_expr(ctx, object)
                    || matches!(
                        crate::type_analysis::static_type_of(ctx, object),
                        Some(HirType::Named(_)) | Some(HirType::Tuple(_))
                    )) =>
        {
            // Scalar-replaced array literal: length is a compile-time
            // constant — no header to load from (the heap array doesn't
            // exist). Must be checked before the cached-length path
            // because scalar-replaced arrays aren't registered there.
            if let Expr::LocalGet(arr_id) = object.as_ref() {
                if let Some(&len) = ctx.non_escaping_arrays.get(arr_id) {
                    return Ok(double_literal(len as f64));
                }
            }
            // Cached-length fast path: when the surrounding for-loop
            // header has hoisted `arr.length` into a stack slot
            // (because it spotted `for (...; i < arr.length; ...)` and
            // proved the body doesn't change `arr.length`), reuse the
            // cached double directly. Without this, the loop body
            // would reload `arr.length` from the array header on every
            // iteration — LLVM's LICM declines to hoist it because the
            // IndexSet's slow path is an opaque external call.
            if let Expr::LocalGet(arr_id) = object.as_ref() {
                if let Some(slot) = ctx.cached_lengths.get(arr_id).cloned() {
                    return Ok(ctx.block().load(DOUBLE, &slot));
                }
            }
            // Issue #73: validate the receiver before the inline load.
            // The compile-time condition above fires for Array / String /
            // Named / Tuple, but TypeScript type erasure (a `Named`-typed
            // binding that ends up holding a plain double; an `unknown[]`
            // whose static analysis resolves back to `Array` at a caller
            // that's actually passing a Buffer/Closure/number) lets
            // non-length-bearing receivers flow in. The existing
            // `safe_load_i32_from_ptr` only catches `handle < 4096`; a
            // denormal double like `0x000000ff_00000000` masks to a
            // ~1TB handle that clears the floor and segfaults the
            // `ldr s0, [handle]`. Two-step guard:
            //
            //   1. Handle must be above the macOS __PAGEZERO region
            //      (4GB). Real mimalloc + arena allocations always
            //      land above this.
            //   2. GC header byte at `handle-8` must indicate
            //      GC_TYPE_ARRAY (1) or GC_TYPE_STRING (3) — the only
            //      two layouts with `length: u32` at payload offset 0.
            //      Buffer / TypedArray don't have GC headers
            //      (they're `std::alloc`'d) so they route through the
            //      runtime slow path, which consults the side-table
            //      registries.
            //
            // Mirrors the v0.5.82 IC-receiver type-validation fix.
            let recv_box = lower_expr(ctx, object)?;
            let blk = ctx.block();
            let recv_bits = blk.bitcast_double_to_i64(&recv_box);
            let recv_handle = blk.and(I64, &recv_bits, POINTER_MASK_I64);
            // Tag-based guard: real heap references carry NaN-box tag
            // POINTER_TAG (0x7FFD) or STRING_TAG (0x7FFF) in the top
            // 16 bits. `AND 0xFFFD` collapses both to 0x7FFD; every
            // other NaN-box / plain double / corrupt bit-pattern
            // (e.g. a `BufferHeader { length: 0, capacity: 255 }`
            // read as u64 → 0x00FF_0000_0000) fails the compare and
            // routes through the slow runtime path.
            //
            // Previously a Darwin mimalloc heap-window check
            // (`> 2 TB && < 128 TB`); aarch64-linux-android Scudo
            // allocations live below 2 TB, so every real array/string
            // was forced through `js_value_length_f64` (issue #128
            // follow-up — correctness-safe, but ~10x slower on the
            // `.length` hot path). Tag check is platform-independent.
            let recv_tag = blk.lshr(I64, &recv_bits, "48");
            let recv_tag_masked = blk.and(I64, &recv_tag, "65533"); // 0xFFFD
            let handle_ok = blk.icmp_eq(I64, &recv_tag_masked, "32765"); // 0x7FFD
                                                                         // SSO receivers fail this guard → route to slow path
                                                                         // `js_value_length_f64` which has an SSO branch (reads
                                                                         // length from the tag byte, no heap access). Accepting
                                                                         // SSO here is safe because the fast path's
                                                                         // `safe_load_i32_from_ptr(&recv_handle)` would read
                                                                         // arbitrary bytes at the SSO "pointer" address, but
                                                                         // the subsequent phi feeds the slow-path result when
                                                                         // handle_ok is false — so SSO flow is correct via the
                                                                         // slow path already, no widening needed.

            let check_gc_idx = ctx.new_block("plen.check_gc");
            let fast_idx = ctx.new_block("plen.fast");
            let slow_idx = ctx.new_block("plen.slow");
            let merge_idx = ctx.new_block("plen.merge");
            let check_gc_label = ctx.block_label(check_gc_idx);
            let fast_label = ctx.block_label(fast_idx);
            let slow_label = ctx.block_label(slow_idx);
            let merge_label = ctx.block_label(merge_idx);
            ctx.block()
                .cond_br(&handle_ok, &check_gc_label, &slow_label);

            ctx.current_block = check_gc_idx;
            let gc_type_addr = ctx.block().sub(I64, &recv_handle, "8");
            let gc_type_ptr = ctx.block().inttoptr(I64, &gc_type_addr);
            let gc_type = ctx.block().load(I8, &gc_type_ptr);
            let is_array = ctx.block().icmp_eq(I8, &gc_type, "1"); // GC_TYPE_ARRAY
            let is_string = ctx.block().icmp_eq(I8, &gc_type, "3"); // GC_TYPE_STRING
            let has_length = ctx.block().or(I1, &is_array, &is_string);
            // Issue #233: a FORWARDED array's first 4 bytes are no
            // longer length but the lower 32 bits of the forwarding
            // pointer. Route those to the slow path
            // (`js_value_length_f64`) which recognizes the flag and
            // follows the chain. GcHeader layout: byte 0 = obj_type,
            // byte 1 = gc_flags. Read the flags byte at handle-7
            // (handle-8 is obj_type) and reject if FORWARDED (0x80).
            let gc_flags_addr = ctx.block().sub(I64, &recv_handle, "7");
            let gc_flags_ptr = ctx.block().inttoptr(I64, &gc_flags_addr);
            let gc_flags = ctx.block().load(I8, &gc_flags_ptr);
            let fwd_bits = ctx.block().and(I8, &gc_flags, "128"); // GC_FLAG_FORWARDED = 0x80
            let not_forwarded = ctx.block().icmp_eq(I8, &fwd_bits, "0");
            let take_fast = ctx.block().and(I1, &has_length, &not_forwarded);
            ctx.block().cond_br(&take_fast, &fast_label, &slow_label);

            ctx.current_block = fast_idx;
            let fast_len_i32 = ctx.block().safe_load_i32_from_ptr(&recv_handle);
            let fast_len = ctx.block().sitofp(I32, &fast_len_i32, DOUBLE);
            let fast_pred_label = ctx.block().label.clone();
            ctx.block().br(&merge_label);

            // Runtime slow path: handles Buffer / TypedArray via side-
            // table registries, returns 0 for non-length-bearing
            // receivers (Closure / BigInt / Promise / Error / plain
            // Object) and for non-pointer NaN-boxes.
            ctx.current_block = slow_idx;
            let slow_len = ctx
                .block()
                .call(DOUBLE, "js_value_length_f64", &[(DOUBLE, &recv_box)]);
            let slow_pred_label = ctx.block().label.clone();
            ctx.block().br(&merge_label);

            ctx.current_block = merge_idx;
            Ok(ctx.block().phi(
                DOUBLE,
                &[(&fast_len, &fast_pred_label), (&slow_len, &slow_pred_label)],
            ))
        }

        // `set.size` / `map.size` — route to runtime helpers. The HIR
        // doesn't synthesize SetSize/MapSize expressions for the
        // property-access form, so we recognize the pattern here.
        Expr::PropertyGet { object, property }
            if property == "size" && is_set_expr(ctx, object) =>
        {
            let recv_box = lower_expr(ctx, object)?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let i32_v = blk.call(I32, "js_set_size", &[(I64, &recv_handle)]);
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        Expr::PropertyGet { object, property }
            if property == "size" && is_map_expr(ctx, object) =>
        {
            let recv_box = lower_expr(ctx, object)?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let i32_v = blk.call(I32, "js_map_size", &[(I64, &recv_handle)]);
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        // Issue #650: `urlSearchParams.size` property — runtime returns
        // i32 length of the internal _entries array. Pre-fix the access
        // fell through to the generic object-field lookup which returned
        // undefined (URLSearchParams stores entries under "_entries", not
        // "size"). Routed via `is_url_search_params_expr` so it only
        // fires on receivers we can prove are URLSearchParams (immediate
        // ctor, typed locals, `url.searchParams` accessor).
        Expr::PropertyGet { object, property }
            if property == "size" && is_url_search_params_expr(ctx, object) =>
        {
            let recv_box = lower_expr(ctx, object)?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let i32_v = blk.call(I32, "js_url_search_params_size", &[(I64, &recv_handle)]);
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }

        // `arr[i] = v` — typed-Number array element write.
        //
        // INLINE FAST PATH:
        //
        //   load length from arr_ptr+0
        //   if idx < length: inline store, done
        //   else if idx < capacity: inline store + bump length, done
        //   else: call js_array_set_f64_extend (slow realloc path)
        //
        // The ArrayHeader layout is `{ length: u32, capacity: u32, ... }`
        // (8 bytes), followed by `[f64; N]` elements at offset 8.
        //
        // For non-LocalGet receivers we still use bounds-checked
        // `js_array_set_f64` (no return value, no realloc) since there's
        // no local to write a possibly-realloc'd pointer back to.
        Expr::PropertyGet { object, property } => {
            // date-fns `constructFrom(date, value)` reads `date.constructor`
            // to clone Dates without naming Date directly. Perry stores
            // Date as a raw f64 timestamp (no ObjectHeader), so the
            // generic `js_object_get_field_by_name_f64` path would treat
            // the bit pattern as an invalid pointer and return undefined.
            // For statically-Date-typed receivers, short-circuit
            // `.constructor` to the global Date constructor closure —
            // same value as the bare `Date` identifier resolves to via
            // `js_get_global_this_builtin_value`.
            if property == "constructor" {
                if let Expr::LocalGet(id) = object.as_ref() {
                    let is_date = matches!(
                        ctx.local_types.get(id),
                        Some(HirType::Named(name)) if name == "Date"
                    );
                    if is_date {
                        let name = "Date";
                        let idx = ctx.strings.intern(name);
                        let bytes_global = format!("@{}", ctx.strings.entry(idx).bytes_global);
                        let len_str = name.len().to_string();
                        return Ok(ctx.block().call(
                            DOUBLE,
                            "js_get_global_this_builtin_value",
                            &[(PTR, &bytes_global), (I64, &len_str)],
                        ));
                    }
                }
            }
            // Issue #649: PropertyGet on a native-module reference (`fs`,
            // `os`, `crypto`, `path`, ...). `NativeModuleRef` lowers to a
            // literal `0.0`, so the generic PropertyGet path can't see the
            // namespace. Short-circuit to `js_native_module_property_by_name`
            // which consults the constants dispatcher directly. For chained
            // access like `fs.constants.F_OK` only the inner read fires
            // here — `constants` returns a real NATIVE_MODULE_CLASS_ID
            // ObjectHeader, and the outer PropertyGet routes through
            // `js_object_get_field_by_name`'s NATIVE_MODULE_CLASS_ID arm.
            if let Expr::NativeModuleRef(module_name) = object.as_ref() {
                if module_name == "process" && property == "version" {
                    let blk = ctx.block();
                    let handle = blk.call(I64, "js_process_version", &[]);
                    return Ok(nanbox_string_inline(blk, &handle));
                }
                let mod_idx = ctx.strings.intern(module_name);
                let mod_bytes_global = format!("@{}", ctx.strings.entry(mod_idx).bytes_global);
                let mod_len_str = module_name.len().to_string();
                let prop_idx = ctx.strings.intern(property);
                let prop_bytes_global = format!("@{}", ctx.strings.entry(prop_idx).bytes_global);
                let prop_len_str = property.len().to_string();
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_native_module_property_by_name",
                    &[
                        (PTR, &mod_bytes_global),
                        (I64, &mod_len_str),
                        (PTR, &prop_bytes_global),
                        (I64, &prop_len_str),
                    ],
                ));
            }
            // Cross-module static field access. When `Base` is an imported
            // class, HIR lowering emits `PropertyGet { ExternFuncRef("Base"),
            // property }` instead of `StaticFieldGet` because the lowering
            // ctx's `class_statics` registry only sees same-module classes.
            // Route through the static-field global map populated from
            // `opts.imported_classes` at codegen entry. Refs #420.
            if let Expr::ExternFuncRef { name, .. } = object.as_ref() {
                let key = (name.clone(), property.clone());
                if let Some(global_name) = ctx.static_field_globals.get(&key).cloned() {
                    let g_ref = format!("@{}", global_name);
                    return Ok(ctx.block().load(DOUBLE, &g_ref));
                }
            }
            // Issue #618-followup: dynamic property access on a local class
            // ref (`SQL.Aliased` after `((SQL2) => { SQL2.Aliased = ...; })(SQL)`).
            // Look up CLASS_DYNAMIC_PROPS via the runtime get-by-name fn,
            // which now detects INT32-tagged class refs at entry. Pass
            // `obj_bits` unmasked so the tag survives.
            //
            // v0.5.757: also handle `Expr::ExternFuncRef` for IMPORTED classes
            // (drizzle's `import { SQL } from "drizzle-orm"`) so
            // `SQL.Aliased` reads via the same dynamic-props path. Without
            // this, the read fell through to the PIC fast path, which
            // discards the INT32 tag during the unbox and ends up returning
            // undefined.
            let is_class_ref_object = matches!(object.as_ref(), Expr::ClassRef(_))
                || matches!(object.as_ref(), Expr::ExternFuncRef { name, .. } if ctx.class_ids.contains_key(name));
            if is_class_ref_object {
                let obj_box = lower_expr(ctx, object)?;
                let key_idx = ctx.strings.intern(property);
                let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let blk = ctx.block();
                let obj_bits = blk.bitcast_double_to_i64(&obj_box);
                let key_box = blk.load(DOUBLE, &key_handle_global);
                let key_bits = blk.bitcast_double_to_i64(&key_box);
                let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                return Ok(blk.call(
                    DOUBLE,
                    "js_object_get_field_by_name_f64",
                    &[(I64, &obj_bits), (I64, &key_raw)],
                ));
            }
            // Scalar replacement fast path: if the receiver is a scalar-replaced
            // local, load directly from the field's alloca — no heap access.
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(slot) = ctx
                    .scalar_replaced
                    .get(id)
                    .and_then(|fs| fs.get(property.as_str()))
                    .cloned()
                {
                    let value = ctx.block().load(DOUBLE, &slot);
                    let lowered = LoweredValue {
                        semantic: SemanticKind::JsValue,
                        rep: NativeRep::JsValue,
                        llvm_ty: DOUBLE,
                        value: value.clone(),
                    };
                    ctx.record_lowered_value_with_access_mode(
                        "ScalarObjectFieldGet",
                        Some(*id),
                        "scalar_object_field_load",
                        &lowered,
                        None,
                        None,
                        None,
                        None,
                        false,
                        false,
                        vec![format!("field={}", property)],
                    );
                    return Ok(value);
                }
                // Issue #613: when the local is scalar-replaced but the
                // property doesn't match any of its known fields, return
                // `undefined` directly. The local's `dummy_slot` doesn't
                // hold a real ObjectHeader pointer (the heap allocation
                // was elided), so falling through to either the
                // runtime helper or the PIC fast path would dereference
                // garbage and SIGTRAP. This matches JS semantics —
                // reading a missing field on a closed-shape object
                // literal must produce `undefined`. The check fires
                // BEFORE the receiver-class fast path because for an
                // any-typed local `const obj: any = { host: "S" }`,
                // `local_types[obj]` is overwritten to the synthetic
                // `__AnonShape_*` class by `Stmt::Let`'s scalar-
                // replacement arm, which would otherwise route the
                // missing-field access through `class_field_global_index`
                // (None for "port") → method-bind check (None) → the
                // generic runtime helper that crashes on the dummy slot.
                if ctx.scalar_replaced.contains_key(id) {
                    return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                }
                // Scalar-replaced array literal: `.length` folds to a
                // compile-time constant. No heap access, no runtime call.
                if property == "length" {
                    if let Some(&len) = ctx.non_escaping_arrays.get(id) {
                        return Ok(double_literal(len as f64));
                    }
                }
            }
            // Also handle `this` during scalar-replaced ctor inlining
            if let Expr::This = object.as_ref() {
                if let Some(slot) = ctx.scalar_ctor_target.last().and_then(|tid| {
                    ctx.scalar_replaced
                        .get(tid)
                        .map(|fs| fs.get(property.as_str()).cloned())
                }) {
                    if let Some(slot) = slot {
                        let value = ctx.block().load(DOUBLE, &slot);
                        let lowered = LoweredValue {
                            semantic: SemanticKind::JsValue,
                            rep: NativeRep::JsValue,
                            llvm_ty: DOUBLE,
                            value: value.clone(),
                        };
                        ctx.record_lowered_value_with_access_mode(
                            "ScalarThisFieldGet",
                            None,
                            "scalar_object_field_load",
                            &lowered,
                            None,
                            None,
                            None,
                            None,
                            false,
                            false,
                            vec![format!("field={}", property)],
                        );
                        return Ok(value);
                    }
                    return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                }
            }
            // GlobalGet receivers (`console.X`, `Math.PI`, `JSON.parse`,
            // `process.env`, …) used as expression VALUES (not in a
            // call) — there's no real value to materialize for most
            // shapes; the call dispatch in lower_call handles the same
            // receivers correctly when they're invoked. The HIR uses
            // `Expr::GlobalGet(0)` as a sentinel for ALL builtin
            // globals (see lower.rs:5037), so the original receiver
            // name is no longer recoverable here — codegen has to
            // route by the property string alone.
            //
            // Special-case `console.log` (the canonical pattern from
            // #236): return a runtime-allocated singleton closure that
            // thunks into `js_console_log_dynamic` so
            // `.then(console.log)` actually prints. Caveat: this also
            // catches the rare `let f = Math.log; f(x)` shape and
            // dispatches through console.log's thunk — but that
            // pattern previously lowered to the `0.0` sentinel
            // (silently broken either way) so this is not a regression
            // for the only realistic alternative caller. The full fix
            // would side-channel the original global name through
            // lowering; deferred until a second-callable-builtin
            // arrives. Other property shapes still fall through to
            // `0.0`.
            if matches!(object.as_ref(), Expr::GlobalGet(_)) {
                if matches!(
                    property.as_str(),
                    "Console"
                        | "log"
                        | "info"
                        | "debug"
                        | "error"
                        | "warn"
                        | "assert"
                        | "dir"
                        | "dirxml"
                        | "trace"
                        | "table"
                        | "clear"
                        | "count"
                        | "countReset"
                        | "time"
                        | "timeEnd"
                        | "timeLog"
                        | "group"
                        | "groupCollapsed"
                        | "groupEnd"
                        | "profile"
                        | "profileEnd"
                        | "timeStamp"
                ) {
                    let mod_idx = ctx.strings.intern("console");
                    let mod_bytes_global = format!("@{}", ctx.strings.entry(mod_idx).bytes_global);
                    let mod_len_str = "console".len().to_string();
                    let prop_idx = ctx.strings.intern(property);
                    let prop_bytes_global =
                        format!("@{}", ctx.strings.entry(prop_idx).bytes_global);
                    let prop_len_str = property.len().to_string();
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_native_module_property_by_name",
                        &[
                            (PTR, &mod_bytes_global),
                            (I64, &mod_len_str),
                            (PTR, &prop_bytes_global),
                            (I64, &prop_len_str),
                        ],
                    ));
                }
                // node:process — `process.abort` / `process.umask` etc. read
                // as VALUES (not called). Bare `process` lowers to the
                // GlobalGet(0) sentinel, so the receiver name is gone here;
                // route by the process-distinctive property name through the
                // native-module property helper, which returns a bound-method
                // closure (typeof "function"). The call forms lower separately
                // via dedicated HIR variants. (#1374, #1373)
                if matches!(
                    property.as_str(),
                    "abort"
                        | "cwd"
                        | "uptime"
                        | "memoryUsage"
                        | "nextTick"
                        | "chdir"
                        | "kill"
                        | "exit"
                        | "umask"
                        | "threadCpuUsage"
                        | "availableMemory"
                        | "constrainedMemory"
                        | "getuid"
                        | "geteuid"
                        | "getgid"
                        | "getegid"
                        | "emitWarning"
                        | "on"
                        | "addListener"
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
                        | "cpuUsage"
                        | "resourceUsage"
                        | "getActiveResourcesInfo"
                        | "hrtime"
                ) {
                    let mod_idx = ctx.strings.intern("process");
                    let mod_bytes_global = format!("@{}", ctx.strings.entry(mod_idx).bytes_global);
                    let mod_len_str = "process".len().to_string();
                    let prop_idx = ctx.strings.intern(property);
                    let prop_bytes_global =
                        format!("@{}", ctx.strings.entry(prop_idx).bytes_global);
                    let prop_len_str = property.len().to_string();
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_native_module_property_by_name",
                        &[
                            (PTR, &mod_bytes_global),
                            (I64, &mod_len_str),
                            (PTR, &prop_bytes_global),
                            (I64, &prop_len_str),
                        ],
                    ));
                }
                // Built-in constructors / namespaces exposed on globalThis
                // (`Array`, `Object`, `Math`, `JSON`, ...): route the read
                // through the singleton so `globalThis.Array` (and the
                // identical `(globalThis as any).X` shape) returns the
                // pre-populated constructor backing-object instead of the
                // `0.0` no-value placeholder. Mirrors the IndexGet arm above
                // (Expr::IndexGet at ~2381) which already routes
                // `globalThis[<string>]` through `js_get_global_this`. The
                // runtime populates these on first init — see
                // `populate_global_this_builtins` in
                // crates/perry-runtime/src/object.rs. Unblocks lodash's
                // `runInContext` (`var Array = context.Array; var arrayProto
                // = Array.prototype`) — the prior `0.0` placeholder caused
                // the `.prototype` chained read on the locally-bound
                // alias to throw `Cannot read properties of undefined`.
                if is_global_this_builtin_name(property) {
                    let global_box = ctx.block().call(DOUBLE, "js_get_global_this", &[]);
                    let key_idx = ctx.strings.intern(property);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let blk = ctx.block();
                    let obj_handle = unbox_to_i64(blk, &global_box);
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                    let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                    return Ok(blk.call(
                        DOUBLE,
                        "js_object_get_field_by_name_f64",
                        &[(I64, &obj_handle), (I64, &key_raw)],
                    ));
                }
                return Ok(double_literal(0.0));
            }
            // Namespace-import member access: `import * as O from './oids';
            // O.OID_INT2`. The HIR lowers `O` itself to `ExternFuncRef { name:
            // "O" }` but `O` isn't a real exported value — it's the namespace
            // binding, so there's no `perry_fn_<src>__O` getter to call. The
            // CLI driver already registers every export of the source module
            // into `import_function_prefixes` under its own name (compile.rs's
            // namespace-import walk), so `O.OID_INT2` just needs to resolve
            // `property` ("OID_INT2") through that map directly and call the
            // same getter a `{ OID_INT2 } from './oids'` named import would
            // have used. Without this, the PropertyGet falls through to the
            // generic path below which lowers the ExternFuncRef "O" to
            // `TAG_TRUE` (the sentinel for unresolved imports) and hands that
            // to `js_object_get_field_by_name_f64` — every namespaced lookup
            // silently returns `undefined`, which is the second half of GH #32
            // (the registry duplication bug was the first).
            if let Expr::ExternFuncRef { name, .. } = object.as_ref() {
                if ctx.namespace_imports.contains(name) {
                    // Issue #841: namespace member access for the five
                    // recognized Node submodules — `import * as ns from
                    // "node:timers/promises"; ns.setTimeout`. Resolve
                    // directly to the per-(submodule, export) function
                    // singleton; same value the named-import would
                    // produce, so `ns.setTimeout === setTimeout` holds.
                    // Done before the class_ids check below because
                    // none of the recognized submodules export classes
                    // by name today; if/when they do (e.g.
                    // `readline/promises.Interface`), the class_ids
                    // branch still wins because class names get
                    // registered into both maps.
                    if let Some(submod_key) = ctx.namespace_node_submodules.get(name) {
                        let submod_label = emit_string_literal_global(ctx, submod_key);
                        let name_label = emit_string_literal_global(ctx, property);
                        let submod_len = submod_key.len();
                        let name_len = property.len();
                        let blk = ctx.block();
                        return Ok(blk.call(
                            DOUBLE,
                            "js_node_submodule_export_as_function",
                            &[
                                (PTR, &submod_label),
                                (I32, &submod_len.to_string()),
                                (PTR, &name_label),
                                (I32, &name_len.to_string()),
                            ],
                        ));
                    }
                    // Issue #574: when the namespace member is itself a class
                    // (`import * as Lib from "./lib"; new Lib.A()` /
                    // `class B extends Lib.A {}`), the export-walk above
                    // registered "A" in both `class_ids` and
                    // `import_function_prefixes`. The function-getter
                    // path below would emit `perry_fn_<src>__A` — but
                    // classes don't have a per-export getter symbol, so
                    // the call returns undefined (silent miss) and
                    // `typeof Lib.A` is "undefined", `Lib.A` reads as
                    // undefined too. Resolve the class reference inline
                    // (mirrors the `Expr::ExternFuncRef` arm at the
                    // bottom of this function): emit the INT32-tagged
                    // class-id NaN-box that `Expr::ClassRef` produces.
                    // #1758: a renamed export (`export { Number$ as Number }`)
                    // is keyed in `class_ids` under the ORIGIN name (`Number$`),
                    // but `property` here is the EXPORTED alias (`Number`). Try
                    // the alias first (direct exports), then the origin name via
                    // `import_function_origin_names` — otherwise `ns.Number`
                    // misses the class ref and falls back to the global
                    // `Number`, dropping all inherited statics (effect's
                    // `S.Number.ast`).
                    let class_cid = ctx.class_ids.get(property).copied().or_else(|| {
                        ctx.import_function_origin_names
                            .get(property)
                            .and_then(|origin| ctx.class_ids.get(origin).copied())
                    });
                    if let Some(cid) = class_cid {
                        let bits = crate::nanbox::INT32_TAG | (cid as u64 & 0xFFFF_FFFF);
                        return Ok(double_literal(f64::from_bits(bits)));
                    }
                    // Issue #680: prefer the per-namespace map so
                    // `random.make` and `tracer.make` resolve to their
                    // own sources even when both modules export `make`.
                    // Falls back to the flat `import_function_prefixes`
                    // for namespaces with no overlapping conflicts.
                    let _ns_lookup_name = if let Expr::ExternFuncRef { name, .. } = object.as_ref()
                    {
                        Some(name.clone())
                    } else {
                        None
                    };
                    let source_prefix_opt = _ns_lookup_name
                        .as_ref()
                        .and_then(|ns| {
                            ctx.namespace_member_prefixes
                                .get(&(ns.clone(), property.clone()))
                                .cloned()
                        })
                        .or_else(|| ctx.import_function_prefixes.get(property).cloned());
                    if let Some(source_prefix) = source_prefix_opt {
                        // Issue #678 followup: V8-fallback namespace member
                        // read as a value (e.g. `let r = ns.render`) — there
                        // is no native getter to call. Return undefined; a
                        // subsequent call goes through the closure-magic check
                        // and fast-paths to undefined. Direct calls of this
                        // shape (`ns.render(...)`) take a different lowering
                        // path that routes through `emit_v8_export_call`.
                        if ctx.import_function_v8_specifiers.contains_key(property) {
                            return Ok(double_literal(f64::from_bits(
                                crate::nanbox::TAG_UNDEFINED,
                            )));
                        }
                        // Issue #671: distinguish exported VARIABLES from
                        // exported FUNCTIONS — for variables, the symbol
                        // `perry_fn_<src>__<prop>` is a trivial getter that
                        // returns the global's value, so calling it with no
                        // args is correct. For functions, `perry_fn_<src>__<prop>`
                        // IS the function body itself; calling it with no args
                        // INVOKES the function (with whatever happened to be in
                        // the arg registers) and returns its result instead of
                        // the function value. Mirrors the var-vs-func split
                        // already used by `Expr::ExternFuncRef` lowering at the
                        // bottom of this function (the `imported_vars` arm at
                        // line ~10432) and by `lower_call.rs:547`'s namespace-
                        // member-CALL path.
                        //
                        // Concrete failure pre-fix (#671): Effect's `HashMap.ts`
                        // top-level binds `keySet = keySet_.keySet`. `keySet`
                        // is an exported `function` declaration in
                        // `internal/hashMap/keySet.ts`, so this arm emitted
                        // `bl perry_fn_..._keySet()` — invoking the keySet
                        // function body with no args during HashMap.ts__init.
                        // The body called `makeImpl` (an imported var from
                        // `internal/hashSet.ts`); with HashMap.ts initialized
                        // before hashSet.ts in the topo order, makeImpl's
                        // global was still 0.0. The 0.0 was handed to
                        // `js_closure_call1` as the closure pointer, tripping
                        // `throw_not_callable` with the literal `value is not
                        // a function`. The fix routes function-shaped namespace
                        // members through `js_closure_alloc_singleton` against
                        // the source's `__perry_wrap_perry_fn_<src>__<prop>`
                        // wrapper — same path the source module's own
                        // `Expr::FuncRef(id)` value-reads use, so the consumer
                        // gets a stable closure handle without invoking the
                        // body. The body only runs later when the consumer
                        // actually calls `HashMap.keySet(self)`, by which time
                        // both modules have finished `__init`.
                        // Issue #678: re-export renames mean the suffix in the
                        // origin module differs from the consumer-visible name.
                        let origin_suffix =
                            import_origin_suffix(ctx.import_function_origin_names, property);
                        if ctx.imported_vars.contains(property) {
                            let getter = format!("perry_fn_{}__{}", source_prefix, origin_suffix);
                            ctx.pending_declares.push((getter.clone(), DOUBLE, vec![]));
                            return Ok(ctx.block().call(DOUBLE, &getter, &[]));
                        }
                        let target_name = format!("perry_fn_{}__{}", source_prefix, origin_suffix);
                        let wrap_name = format!("__perry_wrap_{}", target_name);
                        let param_count = ctx
                            .imported_func_param_counts
                            .get(property)
                            .copied()
                            .unwrap_or(0)
                            .min(5);
                        let mut wrap_param_types: Vec<crate::types::LlvmType> = vec![I64];
                        for _ in 0..param_count {
                            wrap_param_types.push(DOUBLE);
                        }
                        ctx.pending_declares
                            .push((wrap_name.clone(), DOUBLE, wrap_param_types));
                        let blk = ctx.block();
                        let wrap_ptr = format!("@{}", wrap_name);
                        let closure_handle =
                            blk.call(I64, "js_closure_alloc_singleton", &[(PTR, &wrap_ptr)]);
                        return Ok(nanbox_pointer_inline(blk, &closure_handle));
                    }
                }
            }
            // Imported exported-variable access: `Key.DOWN`, `FILTER.X`.
            // ExternFuncRef used as a PropertyGet object means an
            // imported const — call the getter function to load the
            // actual object value, then do the property access on it.
            // Without this, the codegen uses the address of the
            // ClosureHeader global (wrong memory) instead of the
            // object stored in the module's export global.
            if let Expr::ExternFuncRef { name, .. } = object.as_ref() {
                if let Some(source_prefix) = ctx.import_function_prefixes.get(name).cloned() {
                    // Issue #678: re-export renames mean the suffix in the
                    // origin module differs from the consumer-visible name.
                    let origin_suffix =
                        import_origin_suffix(ctx.import_function_origin_names, name);
                    let getter = format!("perry_fn_{}__{}", source_prefix, origin_suffix);
                    ctx.pending_declares.push((getter.clone(), DOUBLE, vec![]));
                    let obj_val = ctx.block().call(DOUBLE, &getter, &[]);
                    // Now do property access on the actual object.
                    let key_idx = ctx.strings.intern(property);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let blk = ctx.block();
                    let obj_bits = blk.bitcast_double_to_i64(&obj_val);
                    let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                    let key_handle = blk.and(I64, &key_bits, POINTER_MASK_I64);
                    return Ok(blk.call(
                        DOUBLE,
                        "js_object_get_field_by_name_f64",
                        &[(I64, &obj_handle), (I64, &key_handle)],
                    ));
                }
            }
            // Getter dispatch: if the receiver is a known class and
            // the property is registered as a getter, call the
            // synthesized __get_<property> method instead of doing a
            // raw field load.
            if let Some(class_name) = receiver_class_name(ctx, object) {
                let getter_key = (class_name.clone(), format!("__get_{}", property));
                if let Some(fn_name) = ctx.methods.get(&getter_key).cloned() {
                    let recv_box = lower_expr(ctx, object)?;
                    return Ok(ctx.block().call(DOUBLE, &fn_name, &[(DOUBLE, &recv_box)]));
                }
                // #1642: bound-method reference for Web Streams instance methods
                // (`typeof rs.getReader === "function"`, `const f = rs.getReader;
                // f()`). Stream instances are numeric handles, not class objects,
                // so the `ctx.methods` path below never matches — bind explicitly
                // via `js_class_method_bind`, whose closure routes calls through
                // `js_native_call_method` → the #1545 stream-handle dispatch. The
                // HIR only routes a stream *method* value-read here (getters keep
                // their 0-arg getter call), so a match here is always a method.
                let is_web_stream_method = matches!(
                    (class_name.as_str(), property.as_str()),
                    (
                        "ReadableStream",
                        "getReader" | "cancel" | "tee" | "pipeTo" | "pipeThrough"
                    ) | (
                        "ReadableStreamDefaultReader",
                        "read" | "releaseLock" | "cancel"
                    ) | ("WritableStream", "getWriter" | "abort" | "close")
                        | (
                            "WritableStreamDefaultWriter",
                            "write" | "close" | "abort" | "releaseLock"
                        )
                );
                if is_web_stream_method {
                    let recv_box = lower_expr(ctx, object)?;
                    let key_idx = ctx.strings.intern(property);
                    let entry = ctx.strings.entry(key_idx);
                    let bytes_global = format!("@{}", entry.bytes_global);
                    let len_str = entry.byte_len.to_string();
                    let blk = ctx.block();
                    let bytes_i64 = blk.ptrtoint(&bytes_global, I64);
                    return Ok(blk.call(
                        DOUBLE,
                        "js_class_method_bind",
                        &[(DOUBLE, &recv_box), (I64, &bytes_i64), (I64, &len_str)],
                    ));
                }
                // Fast path: known class instance + plain instance field
                // (no getter/setter shadowing). Inline a direct GEP+load
                // at the field's slot offset, bypassing the
                // `js_object_get_field_by_name_f64` runtime helper which
                // hashes the property name + walks the keys array. The
                // ObjectHeader layout (`#[repr(C)]` in
                // `crates/perry-runtime/src/object.rs:591`) is 24 bytes
                // followed by the inline field array of f64-sized slots:
                //
                //   offset  0..24:  ObjectHeader (object_type, class_id,
                //                   parent_class_id, field_count, keys_array)
                //   offset 24..32:  field 0
                //   offset 32..40:  field 1
                //   ...
                //
                // Parent class fields come first in the slot order
                // (matches `js_object_alloc_with_parent` and the
                // constructor codegen in lower_call.rs::compile_new), so
                // `class_field_global_index` returns the cumulative
                // offset across the inheritance chain.
                if let Some(field_index) =
                    crate::type_analysis::class_field_global_index(ctx, &class_name, property)
                {
                    if let (Some(&expected_class_id), Some(keys_global_name)) = (
                        ctx.class_ids.get(&class_name),
                        ctx.class_keys_globals.get(&class_name).cloned(),
                    ) {
                        let recv_box = lower_expr(ctx, object)?;
                        let key_idx = ctx.strings.intern(property);
                        let key_handle_global =
                            format!("@{}", ctx.strings.entry(key_idx).handle_global);
                        let site_id = emit_typed_feedback_register_site(
                            ctx,
                            TypedFeedbackKind::PropertyGet,
                            property,
                            TypedFeedbackContract::class_field_get(),
                        );
                        let field_idx_str = field_index.to_string();
                        let expected_class_id_str = expected_class_id.to_string();
                        let requires_raw_f64 = crate::type_analysis::class_field_declared_type(
                            ctx,
                            &class_name,
                            property,
                        )
                        .as_ref()
                        .is_some_and(crate::typed_shape::type_is_raw_f64_candidate);
                        let requires_raw_f64_str = if requires_raw_f64 { "1" } else { "0" };
                        let (obj_bits, obj_handle, key_raw, guard_ok) = {
                            let blk = ctx.block();
                            let obj_bits = blk.bitcast_double_to_i64(&recv_box);
                            let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                            let key_box = blk.load(DOUBLE, &key_handle_global);
                            let key_bits = blk.bitcast_double_to_i64(&key_box);
                            let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                            let expected_keys = blk.load(I64, &format!("@{}", keys_global_name));
                            let guard_ok = blk.call(
                                I32,
                                "js_typed_feedback_class_field_get_guard",
                                &[
                                    (I64, &site_id),
                                    (DOUBLE, &recv_box),
                                    (I32, &expected_class_id_str),
                                    (I64, &expected_keys),
                                    (I64, &key_raw),
                                    (I32, &field_idx_str),
                                    (I32, requires_raw_f64_str),
                                ],
                            );
                            (obj_bits, obj_handle, key_raw, guard_ok)
                        };
                        let guard_pass = ctx.block().icmp_ne(I32, &guard_ok, "0");
                        let fast_idx = ctx.new_block("class_field_get.fast");
                        let fallback_idx = ctx.new_block("class_field_get.fallback");
                        let merge_idx = ctx.new_block("class_field_get.merge");
                        let fast_label = ctx.block_label(fast_idx);
                        let fallback_label = ctx.block_label(fallback_idx);
                        let merge_label = ctx.block_label(merge_idx);
                        ctx.block()
                            .cond_br(&guard_pass, &fast_label, &fallback_label);

                        ctx.current_block = fast_idx;
                        let blk = ctx.block();
                        let obj_ptr = blk.inttoptr(I64, &obj_handle);
                        // Skip the 24-byte ObjectHeader.
                        let header_skip = "24".to_string();
                        let fields_base = blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
                        let field_ptr = blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
                        let val_fast = blk.load(DOUBLE, &field_ptr);
                        let fast_end_label = blk.label.clone();
                        blk.br(&merge_label);
                        if requires_raw_f64 {
                            let fast = LoweredValue {
                                semantic: SemanticKind::JsNumber,
                                rep: NativeRep::F64,
                                llvm_ty: DOUBLE,
                                value: val_fast.clone(),
                            };
                            ctx.record_lowered_value_with_access_mode_and_facts(
                                "ClassFieldGet",
                                None,
                                "class_field_get.raw_f64_load",
                                &fast,
                                Some(BoundsState::Guarded {
                                    guard_id: "class_field_get_guard".to_string(),
                                }),
                                None,
                                Some(BufferAccessMode::CheckedNative),
                                None,
                                None,
                                None,
                                vec![raw_f64_layout_fact(
                                    None,
                                    "consumed",
                                    "class_field_get_guard",
                                    None,
                                )],
                                Vec::new(),
                                false,
                                false,
                                vec![
                                    format!("class={}", class_name),
                                    format!("field={}", property),
                                    format!("field_index={}", field_idx_str),
                                ],
                            );
                        }

                        ctx.current_block = fallback_idx;
                        let blk = ctx.block();
                        blk.call_void("js_typed_feedback_record_fallback_call", &[(I64, &site_id)]);
                        let val_fallback = blk.call(
                            DOUBLE,
                            "js_object_get_field_by_name_f64",
                            &[(I64, &obj_bits), (I64, &key_raw)],
                        );
                        let fallback_end_label = blk.label.clone();
                        blk.br(&merge_label);
                        if requires_raw_f64 {
                            let fallback = LoweredValue {
                                semantic: SemanticKind::JsValue,
                                rep: NativeRep::JsValue,
                                llvm_ty: DOUBLE,
                                value: val_fallback.clone(),
                            };
                            ctx.record_lowered_value_with_access_mode_and_facts(
                                "ClassFieldGet",
                                None,
                                "js_object_get_field_by_name_f64",
                                &fallback,
                                Some(BoundsState::Unknown),
                                None,
                                Some(BufferAccessMode::DynamicFallback),
                                Some(MaterializationReason::RuntimeApi),
                                None,
                                None,
                                Vec::new(),
                                vec![
                                    raw_f64_layout_fact(
                                        None,
                                        "rejected",
                                        "class_field_get_guard",
                                        Some(MaterializationReason::RuntimeApi),
                                    ),
                                    raw_f64_layout_fact(
                                        None,
                                        "invalidated",
                                        "runtime_api",
                                        Some(MaterializationReason::RuntimeApi),
                                    ),
                                ],
                                false,
                                false,
                                vec![
                                    format!("class={}", class_name),
                                    format!("field={}", property),
                                    format!("field_index={}", field_idx_str),
                                ],
                            );
                        }

                        ctx.current_block = merge_idx;
                        return Ok(ctx.block().phi(
                            DOUBLE,
                            &[
                                (&val_fast, &fast_end_label),
                                (&val_fallback, &fallback_end_label),
                            ],
                        ));
                    }
                }
                // Issue #446: `obj.method` PropertyGet on a known class
                // instance, where `method` is a method (not a field, not a
                // getter — those branches return above). Emit a bound-method
                // closure (`BOUND_METHOD_FUNC_PTR` sentinel + (instance,
                // name_ptr, name_len) captures) so reads work as JS expects:
                //   - `typeof obj.method === "function"`
                //   - `let f = obj.method; f(args)` dispatches to the method
                //   - `arr.map(obj.method)` passes a callable reference
                // The closure's call path routes through
                // `js_native_call_method`, which resolves the symbol via
                // `CLASS_VTABLE_REGISTRY` (populated at module init by
                // `js_register_class_method`), so this works for both local
                // and cross-module classes. Pre-fix, the read fell through
                // to the generic property-bag lookup which doesn't store
                // prototype methods — every method reference returned
                // `undefined`.
                let method_key = (class_name.clone(), property.clone());
                if ctx.methods.contains_key(&method_key) {
                    let recv_box = lower_expr(ctx, object)?;
                    let key_idx = ctx.strings.intern(property);
                    let entry = ctx.strings.entry(key_idx);
                    let bytes_global = format!("@{}", entry.bytes_global);
                    let len_str = entry.byte_len.to_string();
                    let blk = ctx.block();
                    let bytes_i64 = blk.ptrtoint(&bytes_global, I64);
                    return Ok(blk.call(
                        DOUBLE,
                        "js_class_method_bind",
                        &[(DOUBLE, &recv_box), (I64, &bytes_i64), (I64, &len_str)],
                    ));
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
            let feedback_site_id = emit_typed_feedback_register_site(
                ctx,
                TypedFeedbackKind::PropertyGet,
                property,
                TypedFeedbackContract::object_get_by_name(),
            );

            // Issue #70/#73/#128: guard against non-pointer receivers
            // before the PIC deref. Tag-based check on the unmasked
            // NaN-box: real heap references have high-16-bits POINTER_TAG
            // (0x7FFD) or STRING_TAG (0x7FFF). `AND 0xFFFD` collapses both
            // to 0x7FFD; everything else (undefined/null/bool=0x7FFC,
            // int32=0x7FFE, bigint=0x7FFA, plain f64 like 0.0 globalThis
            // or 3.14, corrupt bit-patterns like 0x00FF_0000_0000 read as
            // a BufferHeader) falls through to the invalid branch and
            // returns undefined safely.
            //
            // Previously used a Darwin mimalloc heap-window check
            // (`> 2 TB && < 128 TB`). On aarch64-linux-android (issue
            // #128) Bionic Scudo allocations live far below 2 TB, so
            // every real object pointer failed the guard and the IC
            // returned undefined — `obj.x` read as NaN everywhere,
            // silently corrupting FFI args and pure-TS field compares.
            // Tag check is platform-independent: same two LLVM ops
            // (`lshr` + `and`) + one `icmp`, branch-predicted taken.
            let obj_tag = ctx.block().lshr(I64, &obj_bits, "48");
            // SSO receiver fast path (Step 1.5 of SSO migration).
            // SHORT_STRING_TAG = 0x7FF9 can't pass the POINTER/STRING
            // check (its masked tag is 0x7FF9, not 0x7FFD) and we
            // can't widen the mask because the PIC fast path's
            // `*(obj_handle + 16)` would read arbitrary memory from
            // the SSO data bits. Instead: check SSO explicitly first,
            // route to a dedicated block that calls the SSO-aware
            // `js_object_get_field_by_name_f64` runtime entry (which
            // handles `.length` directly from the NaN-box length
            // byte and returns `undefined` for other keys).
            let is_sso = ctx.block().icmp_eq(I64, &obj_tag, "32761"); // 0x7FF9
                                                                      // v0.5.747: INT32-tagged class refs (top16 == 0x7FFE) used
                                                                      // as PropertyGet receivers. Pre-fix these fell through to
                                                                      // the invalid-recv path (returning undefined) because the
                                                                      // 0xFFFD-masked tag check (0x7FFE & 0xFFFD = 0x7FFC, not
                                                                      // 0x7FFD) treated them as non-pointer values. Drizzle's
                                                                      // `is(value, type)` chain depends on `Cls.kind` reads through
                                                                      // an Any-typed local. Refs #420 / #618 followup.
                                                                      //
                                                                      // Note: this also catches plain int32 numeric values (e.g.
                                                                      // `(42).property`). The runtime helper's INT32-tag arm at
                                                                      // js_object_get_field_by_name returns undefined for any
                                                                      // class_id not registered in CLASS_DYNAMIC_PROPS, matching
                                                                      // the previous behavior — pure ints have no static fields.
            let is_int32_class = ctx.block().icmp_eq(I64, &obj_tag, "32766"); // 0x7FFE
            let obj_tag_masked = ctx.block().and(I64, &obj_tag, "65533"); // 0xFFFD
            let is_valid = ctx.block().icmp_eq(I64, &obj_tag_masked, "32765"); // 0x7FFD
            let sso_idx = ctx.new_block("pget.recv_sso");
            let pic_idx = ctx.new_block("pget.recv_ok");
            let invalid_idx = ctx.new_block("pget.recv_bad");
            let class_ref_idx = ctx.new_block("pget.recv_class_ref");
            let final_merge_idx = ctx.new_block("pget.recv_merge");
            let sso_label = ctx.block_label(sso_idx);
            let pic_label = ctx.block_label(pic_idx);
            let invalid_label = ctx.block_label(invalid_idx);
            let class_ref_label = ctx.block_label(class_ref_idx);
            let final_merge_label = ctx.block_label(final_merge_idx);
            // Three-step branch: first check SSO, then class-ref, then
            // pointer-validity. Inverse branches funnel into invalid_idx.
            let pic_or_invalid_idx = ctx.new_block("pget.check_ptr");
            let pic_or_invalid_label = ctx.block_label(pic_or_invalid_idx);
            let check_class_ref_idx = ctx.new_block("pget.check_class_ref");
            let check_class_ref_label = ctx.block_label(check_class_ref_idx);
            ctx.block()
                .cond_br(&is_sso, &sso_label, &check_class_ref_label);
            ctx.current_block = check_class_ref_idx;
            ctx.block()
                .cond_br(&is_int32_class, &class_ref_label, &pic_or_invalid_label);
            ctx.current_block = pic_or_invalid_idx;
            ctx.block().cond_br(&is_valid, &pic_label, &invalid_label);

            // Class-ref dispatch: route through the runtime helper which
            // detects INT32 class-ref bits and consults CLASS_DYNAMIC_PROPS
            // for the static field / dynamic IIFE-set property / synthetic
            // `constructor` lookup. Pass full obj_bits (NOT obj_handle —
            // the runtime needs the unmasked top16 to detect the tag).
            ctx.current_block = class_ref_idx;
            let class_ref_result = ctx.block().call(
                DOUBLE,
                "js_typed_feedback_object_get_field_by_name_f64",
                &[
                    (I64, &feedback_site_id),
                    (I64, &obj_bits),
                    (I64, &key_handle),
                ],
            );
            let class_ref_end_label = ctx.block().label.clone();
            ctx.block().br(&final_merge_label);

            ctx.current_block = pic_idx;
            ctx.block().call_void(
                "js_typed_feedback_observe_property_get",
                &[
                    (I64, &feedback_site_id),
                    (I64, &obj_handle),
                    (I64, &key_handle),
                ],
            );

            // Issue #51: monomorphic inline cache. Per-site 16-byte global
            // holds [cached_keys_array_ptr, cached_slot_index]. The fast path
            // compares obj->keys_array (offset 16) to cache[0]; on match,
            // loads the field directly at obj+24+slot*8 — no function call,
            // no hash, no linear scan. On miss, calls the slow helper which
            // does the full lookup and primes the cache for next time.
            let site_id = ctx.ic_site_counter;
            ctx.ic_site_counter += 1;
            let cache_name = format!("perry_ic_{}", site_id);
            ctx.pending_declares
                .push((format!("__ic_decl_{}", site_id), DOUBLE, vec![]));
            ctx.ic_globals.push(cache_name.clone());

            // Issue #72: validate the receiver is actually a GC_TYPE_OBJECT
            // before treating offset 16 as `keys_array`. The v0.5.78 receiver
            // guard (`obj_handle > 0x100000`) keeps non-pointer NaN-boxes out,
            // but real heap pointers to Arrays/Strings/Buffers all clear that
            // threshold. A chained `obj.rowsRaw.length` (whose static type
            // analysis can't prove `obj.rowsRaw` is an Array — the outer
            // PropertyGet falls into this generic dispatch) hands the array's
            // pointer to this PIC. For an Array, offset 16 is element[1]; on
            // a freshly-allocated array element[1] is zero, the per-site
            // cache global is zero-initialized, so the keys_val comparison
            // falsely "hits" and the hit-path loads (obj+24+slot*8) — i.e.
            // element[2] — as the field value, returning 0 instead of
            // dispatching `.length`. The slow `js_object_get_field_by_name`
            // already routes by `gc_type` (handles Array.length, String.length,
            // Set.size, Buffer.length, Error.message, etc.), so funneling
            // non-OBJECT receivers through the miss handler fixes correctness
            // without giving up the PIC for real objects.
            //
            // Issue #340/#341: small-handle guard. Receivers from
            // native modules (axios, fastify, ioredis, better-sqlite3,
            // ...) are NaN-boxed POINTER values whose lower-48 is a
            // small registry id (1, 2, 3, ...). The PIC fast path
            // below deref's `obj_handle - 8` for the GcHeader byte
            // and `obj_handle + 16` for the keys_array slot — both
            // SIGSEGV when `obj_handle` is a small int. Funnel
            // small-handle receivers through the slow path so they
            // reach the runtime's `HANDLE_PROPERTY_DISPATCH` table
            // (axios `r.status` / `r.data`, fastify `req.query` /
            // `req.params`, etc.).
            //
            // Threshold matches `js_native_call_method`'s small-handle
            // detection (raw_ptr < 0x100000) and `js_object_get_field_by_name`'s
            // post-#340 fix that calls HANDLE_PROPERTY_DISPATCH for
            // these receivers.
            // Issue #340/#341: small-handle guard. Receivers from
            // native modules (axios, fastify, ioredis, better-sqlite3,
            // ...) are NaN-boxed POINTER values whose lower-48 is a
            // small registry id (1, 2, 3, ...). The PIC fast path
            // below deref's `obj_handle - 8` for the GcHeader byte
            // and `obj_handle + 16` for the keys_array slot — both
            // SIGSEGV when `obj_handle` is a small int. Use a select
            // to swap in a known-safe address (the per-site cache
            // global itself) for the load, then AND `is_real_ptr`
            // into the hit predicate so handle receivers cleanly
            // miss to the slow path. The slow path
            // (`js_object_get_field_ic_miss` →
            // `js_object_get_field_by_name`) routes handles to
            // `HANDLE_PROPERTY_DISPATCH` (axios `r.status` / `r.data`,
            // fastify `req.query`, etc.).
            //
            // Threshold matches `js_native_call_method`'s small-handle
            // detection (raw_ptr < 0x100000).
            let is_real_ptr = ctx.block().icmp_ugt(I64, &obj_handle, "1048575"); // 0x100000

            // Sentinel address: the per-site cache global itself —
            // always valid, 16-byte aligned, and its bytes don't
            // match GC_TYPE_OBJECT (=2) or an active keys_array, so
            // the IC will cleanly miss when we substitute it for a
            // small handle.
            let cache_ref = format!("@{}", cache_name);
            let cache_addr = ctx.block().ptrtoint(&cache_ref, I64);
            let safe_obj_handle =
                ctx.block()
                    .select(I1, &is_real_ptr, I64, &obj_handle, &cache_addr);

            // GcHeader sits 8 bytes before the user pointer; obj_type is the
            // first u8 (GC_TYPE_OBJECT=2). Cost: 1 sub + 1 load i8 + 1 cmp
            // i8 + 1 and i1 — the cond_br's `is_object` operand is folded
            // into the existing branch instruction by LLVM. Branch-predicted
            // taken since real PropertyGet receivers are objects.
            let gc_type_addr = ctx.block().sub(I64, &safe_obj_handle, "8");
            let gc_type_ptr = ctx.block().inttoptr(I64, &gc_type_addr);
            let gc_type = ctx.block().load(I8, &gc_type_ptr);
            let gc_type_ok = ctx.block().icmp_eq(I8, &gc_type, "2");
            let is_object = ctx.block().and(I1, &is_real_ptr, &gc_type_ok);

            // Issue #618: closures share GC_TYPE_OBJECT but their offset+16
            // is a capture slot, not `keys_array`. The PIC's keys_val ==
            // cached_keys check would spuriously hit (per-site cache global
            // is zero-initialized; capture[0] of a 0-capture wrapper is also
            // often zero) and the hit path would load garbage from the
            // capture region. Detect CLOSURE_MAGIC at +12 and force the
            // PIC to miss for closures so the read routes through
            // `js_object_get_field_ic_miss` → `js_object_get_field_by_name`,
            // which dispatches closure dynamic-prop reads via the
            // `CLOSURE_DYNAMIC_PROPS` side-table.
            let magic_addr = ctx.block().add(I64, &safe_obj_handle, "12");
            let magic_ptr = ctx.block().inttoptr(I64, &magic_addr);
            let magic_val = ctx.block().load(I32, &magic_ptr);
            // CLOSURE_MAGIC = 0x434C4F53 (4 bytes "CLOS" little-endian).
            let is_closure = ctx.block().icmp_eq(I32, &magic_val, "1129268819");
            let not_closure = ctx.block().xor(I1, &is_closure, "true");
            let is_object = ctx.block().and(I1, &is_object, &not_closure);

            // Issue #637: RegExpHeader / PromiseHeader / MapHeader / SetHeader
            // / TypedArrayHeader / ... all share GC_TYPE_OBJECT but have
            // different layouts than ObjectHeader. The first u32 of an
            // ObjectHeader is `object_type = OBJECT_TYPE_REGULAR (=1)`;
            // for these other headers the first 4 bytes are part of a
            // pointer or method table, almost never 1. Without this check,
            // a PIC site that learned a real ObjectHeader's [keys_array,
            // slot] cache could spuriously hit on a regex/promise/etc.
            // whose offset-16 happens to match (e.g. both null flags_ptr
            // and uninitialized cache[0] are 0), and the hit path would
            // load garbage from offset 24 of the non-Object header.
            // Specific repro: `function f(): any { ... return new
            // RegExp(...) } const r = f(); r.source` — fast path returns
            // garbage f64 instead of routing through `js_regexp_get_source`.
            let object_type_ptr = ctx.block().inttoptr(I64, &safe_obj_handle);
            let object_type = ctx.block().load(I32, &object_type_ptr);
            let object_type_ok = ctx.block().icmp_eq(I32, &object_type, "1");
            let is_object = ctx.block().and(I1, &is_object, &object_type_ok);

            // Load obj->keys_array at offset 16 of ObjectHeader.
            let keys_addr = ctx.block().add(I64, &safe_obj_handle, "16");
            let keys_ptr_p = ctx.block().inttoptr(I64, &keys_addr);
            let keys_val = ctx.block().load(I64, &keys_ptr_p);

            // Load cached keys_array from the per-site global.
            let cache_keys_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "0")]);
            let cached_keys = ctx.block().load(I64, &cache_keys_ptr);
            let keys_eq = ctx.block().icmp_eq(I64, &keys_val, &cached_keys);
            // #809: an object with `keys_array == null` (e.g. an
            // `Object.create(proto)` result, or any object with no own
            // string props) has no cacheable own-slot. The per-site cache
            // global is zero-initialized, so `keys_val (0) == cached_keys
            // (0)` spuriously "hits" and the hit path returns the empty
            // slot[0] — never invoking the miss handler, so the runtime's
            // prototype-chain walk in `js_object_get_field_by_name` is
            // skipped and `Object.create(P).m()` reads `undefined`. Require
            // a non-null keys_array for a hit so keyless receivers fall to
            // the slow path (which resolves inherited props correctly).
            let keys_nonnull = ctx.block().icmp_ne(I64, &keys_val, "0");
            let hit_keys = ctx.block().and(I1, &is_object, &keys_eq);
            let hit = ctx.block().and(I1, &hit_keys, &keys_nonnull);

            let hit_idx = ctx.new_block("pic.hit");
            let miss_idx = ctx.new_block("pic.miss");
            let merge_idx = ctx.new_block("pic.merge");
            let hit_label = ctx.block_label(hit_idx);
            let miss_label = ctx.block_label(miss_idx);
            let merge_label = ctx.block_label(merge_idx);
            ctx.block().cond_br(&hit, &hit_label, &miss_label);

            // PIC hit: direct field load.
            ctx.current_block = hit_idx;
            ctx.block().call_void(
                "js_typed_feedback_record_guard_pass",
                &[(I64, &feedback_site_id)],
            );
            let cache_slot_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "1")]);
            let slot = ctx.block().load(I64, &cache_slot_ptr);
            let offset = ctx.block().shl(I64, &slot, "3");
            let base = ctx.block().add(I64, &obj_handle, "24");
            let field_addr = ctx.block().add(I64, &base, &offset);
            let field_ptr = ctx.block().inttoptr(I64, &field_addr);
            let val_hit = ctx.block().load(DOUBLE, &field_ptr);
            let hit_end_label = ctx.block().label.clone();
            ctx.block().br(&merge_label);

            // PIC miss: slow path with cache population.
            ctx.current_block = miss_idx;
            ctx.block().call_void(
                "js_typed_feedback_record_guard_fail",
                &[(I64, &feedback_site_id)],
            );
            ctx.block().call_void(
                "js_typed_feedback_record_fallback_call",
                &[(I64, &feedback_site_id)],
            );
            let val_miss = ctx.block().call(
                DOUBLE,
                "js_object_get_field_ic_miss",
                &[(I64, &obj_handle), (I64, &key_handle), (PTR, &cache_ref)],
            );
            let miss_end_label = ctx.block().label.clone();
            ctx.block().br(&merge_label);

            // Merge PIC hit + miss, then jump to the outer recv-valid merge.
            ctx.current_block = merge_idx;
            let pic_val = ctx.block().phi(
                DOUBLE,
                &[(&val_hit, &hit_end_label), (&val_miss, &miss_end_label)],
            );
            let pic_end_label = ctx.block().label.clone();
            ctx.block().br(&final_merge_label);

            // Invalid receiver: per JS spec, `undefined` and `null`
            // throw a TypeError; other non-pointer tags (int32, bool,
            // plain f64, bigint) should auto-box and look up via the
            // primitive's prototype. Perry doesn't implement primitive
            // auto-boxing yet, so non-nullish primitives continue to
            // return `undefined` to preserve existing behavior.
            //
            // Issue #462: bare `obj.foo` against TAG_UNDEFINED /
            // TAG_NULL silently returned undefined, which masked
            // unimplemented-API bugs (e.g. `crypto.subtle.encrypt(...)`
            // ran to completion as a chain of no-ops). Funnel the
            // nullish receiver into the runtime helper which prints a
            // node-shaped diagnostic and aborts.
            ctx.current_block = invalid_idx;
            let is_undef = ctx
                .block()
                .icmp_eq(I64, &obj_bits, crate::nanbox::TAG_UNDEFINED_I64);
            let is_null = ctx
                .block()
                .icmp_eq(I64, &obj_bits, crate::nanbox::TAG_NULL_I64);
            let is_nullish = ctx.block().or(I1, &is_undef, &is_null);
            let throw_idx = ctx.new_block("pget.throw_nullish");
            let undef_idx = ctx.new_block("pget.recv_undef_return");
            let throw_label = ctx.block_label(throw_idx);
            let undef_label = ctx.block_label(undef_idx);
            ctx.block().cond_br(&is_nullish, &throw_label, &undef_label);

            // Throw path: helper aborts the process; block ends with
            // `unreachable` because the helper's `-> !` return is
            // not visible to LLVM.
            ctx.current_block = throw_idx;
            let prop_entry = ctx.strings.entry(key_idx);
            let prop_bytes_global = format!("@{}", prop_entry.bytes_global);
            let prop_len_str = prop_entry.byte_len.to_string();
            let is_null_i32 = ctx.block().zext(I1, &is_null, I32);
            ctx.block().call_void(
                "js_throw_type_error_property_access",
                &[
                    (I32, &is_null_i32),
                    (PTR, &prop_bytes_global),
                    (I64, &prop_len_str),
                ],
            );
            ctx.block().unreachable();

            // Undef-return path: existing fall-through for non-nullish
            // invalid receivers. Route through the runtime helper first
            // so non-pointer typed shapes can still report a sensible
            // value when the runtime knows what they are. Today this
            // unblocks Date `.constructor` (Date stores as a raw f64
            // timestamp, so the codegen receiver-tag check at line ~4212
            // rejects it as non-pointer — yet the runtime's
            // `js_object_get_field_by_name_f64` recognizes the bit
            // pattern via `DATE_REGISTRY` and returns the global Date
            // constructor closure). Date-fns `constructFrom` blocker.
            ctx.current_block = undef_idx;
            let undef_val = ctx.block().call(
                DOUBLE,
                "js_object_get_field_by_name_f64",
                &[(I64, &obj_bits), (I64, &key_handle)],
            );
            let invalid_end_label = ctx.block().label.clone();
            ctx.block().br(&final_merge_label);

            // SSO receiver: dispatch directly to the runtime by-name
            // helper, which reads `.length` inline from the NaN-box
            // payload and returns `undefined` for other keys. Bypasses
            // the PIC entirely (PIC would read garbage memory). The
            // key handle has already been extracted above.
            ctx.current_block = sso_idx;
            let sso_val = ctx.block().call(
                DOUBLE,
                "js_object_get_field_by_name_f64",
                &[(I64, &obj_bits), (I64, &key_handle)],
            );
            let sso_end_label = ctx.block().label.clone();
            ctx.block().br(&final_merge_label);

            // Outer merge joins PIC result + invalid-receiver undefined
            // + SSO result + class-ref dispatch result.
            ctx.current_block = final_merge_idx;
            Ok(ctx.block().phi(
                DOUBLE,
                &[
                    (&pic_val, &pic_end_label),
                    (&undef_val, &invalid_end_label),
                    (&sso_val, &sso_end_label),
                    (&class_ref_result, &class_ref_end_label),
                ],
            ))
        }

        // -------- Ternary `cond ? a : b` (Phase B.7) --------
        // Lowered like if-expression with phi merge — same shape as the
        // logical operator path but with both branches always evaluated
        // conditionally on the truthiness test.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
