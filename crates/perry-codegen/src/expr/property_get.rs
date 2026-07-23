//! PropertyGet — guarded specializations + general catchall.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::native_value::{
    BoundsState, BufferAccessMode, LoweredValue, MaterializationReason, NativeRep, SemanticKind,
};
use crate::type_analysis::{
    is_array_expr, is_map_expr, is_numeric_typed_array_class, is_set_expr, is_string_expr,
    is_url_search_params_expr, is_url_search_params_subclass_expr, receiver_class_name,
    receiver_is_error_type,
};
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

use super::property_get_names::{
    is_headers_method_name, is_http_agent_method_name, is_http_client_request_method_name,
    is_net_native_method_value, is_url_pattern_data_property,
};

mod generic_dispatch;
mod globalget;
mod helpers;
#[cfg(test)]
mod nullish_read_location_tests;

pub(crate) use generic_dispatch::lower_generic_property_get;
pub(crate) use globalget::lower_globalget_property;
pub(crate) use helpers::{
    builtin_prototype_method_read, class_has_computed_runtime_members,
    is_global_builtin_value_expr, lower_class_method_bind, lower_global_builtin_static_value,
    lower_raw_f64_class_field_get_for_number_context, lower_runtime_property_get_by_name,
    promise_static_function_length_expr,
};

use super::{
    emit_string_literal_global, emit_typed_feedback_register_site, import_origin_suffix,
    import_origin_suffix_ns, is_global_this_builtin_name, lower_expr, nanbox_pointer_inline,
    nanbox_string_inline, raw_f64_layout_fact, try_lower_pod_field_get, unbox_to_i64, FnCtx,
    TypedFeedbackContract, TypedFeedbackKind,
};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    // `split("literal")[constant].length` on a scalar-replaced split can
    // read the precomputed numeric length directly. The split part itself was
    // never observable as a string, so materializing a StringHeader would only
    // create short-lived garbage.
    if let Expr::PropertyGet {
        object, property, ..
    } = expr
    {
        if property == "length" {
            if let Expr::IndexGet { object, index } = object.as_ref() {
                if let (Expr::LocalGet(id), Some(index)) =
                    (object.as_ref(), crate::collectors::const_index(index))
                {
                    if let Some(slot) = ctx
                        .scalar_replaced_split_part_lengths
                        .get(id)
                        .and_then(|lengths| lengths.get(&index))
                        .cloned()
                    {
                        return Ok(ctx.block().load(DOUBLE, &slot));
                    }
                }
            }
        }
    }

    match expr {
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::LocalGet(id)
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
        Expr::PropertyGet {
            object, property, ..
        } if property == "length"
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
            Ok(blk.uitofp(I32, &len_i32, DOUBLE))
        }

        // Phase H err: `agg.errors` — AggregateError.errors field.
        // Routes through js_error_get_errors which pulls the raw
        // ArrayHeader pointer from the ErrorHeader struct. Returns a
        // NaN-boxed pointer so downstream length / index operations
        // see an array.
        //
        // Gated on a statically-known Error receiver (#6588): the helper's
        // `ArrayHeader*` return can't represent a stored `null`, so applying
        // it to a function/plain-object `.errors` expando that holds `null`
        // produced a bogus pointer sentinel (`f.errors === null` → false,
        // `String(f.errors)` → "[object Object]"). Non-error receivers fall
        // through to the generic property read below, which returns the
        // stored value — including `null` — correctly.
        Expr::PropertyGet {
            object, property, ..
        } if property == "errors" && receiver_is_error_type(ctx, object) => {
            let recv_box = lower_expr(ctx, object)?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let arr_handle = blk.call(I64, "js_error_get_errors", &[(I64, &recv_handle)]);
            Ok(nanbox_pointer_inline(blk, &arr_handle))
        }

        Expr::PropertyGet {
            object, property, ..
        } if is_global_builtin_value_expr(object, "Promise")
            && matches!(
                property.as_str(),
                "resolve"
                    | "reject"
                    | "all"
                    | "race"
                    | "allSettled"
                    | "any"
                    | "withResolvers"
                    | "try"
            ) =>
        {
            Ok(lower_global_builtin_static_value(ctx, "Promise", property))
        }

        Expr::PropertyGet {
            object, property, ..
        } if property == "length" && promise_static_function_length_expr(object).is_some() => {
            let len = promise_static_function_length_expr(object).unwrap();
            Ok(double_literal(len as f64))
        }

        Expr::PropertyGet {
            object, property, ..
        } if property == "length"
            && matches!(object.as_ref(), Expr::LocalGet(id)
                    if ctx.buffer_data_slots.contains_key(id)) =>
        {
            let arr_id = match object.as_ref() {
                Expr::LocalGet(id) => *id,
                _ => unreachable!(),
            };
            let (ptr_slot, _scope) = ctx.buffer_data_slots.get(&arr_id).cloned().unwrap();
            // The length field's byte offset relative to `data_ptr` differs by
            // header layout: an 8-byte `BufferHeader` keeps it at `data-8`, but
            // a 16-byte `TypedArrayHeader` (Int32Array/Float64Array/... numeric
            // -length constructors) keeps it at `data-16`. #1862 began
            // registering multi-byte typed arrays in `buffer_data_slots` with a
            // data_ptr 16 bytes past the header, so the hardcoded `-8` here read
            // the packed `kind|elem_size` bytes (Int32→0x404=1028,
            // Float64→0x807=2055) instead of `.length`. Prefer the co-registered
            // `buffer_view_slots` entry, which carries the correct
            // `length_offset_from_data` (and a `length_slot` for native views).
            let view = ctx.buffer_view_slots.get(&arr_id).cloned();
            let length_slot = view.as_ref().and_then(|v| v.length_slot.clone());
            let length_offset = view
                .as_ref()
                .map(|v| v.length_offset_from_data)
                .unwrap_or(-8);
            let blk = ctx.block();
            let len_i32 = if let Some(length_slot) = length_slot.as_ref() {
                blk.load(I32, length_slot)
            } else {
                let data_ptr = blk.load(PTR, &ptr_slot);
                let header_ptr = blk.gep(I8, &data_ptr, &[(I32, &length_offset.to_string())]);
                blk.load_invariant(I32, &header_ptr)
            };
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
            Ok(crate::native_value::materialize_js_value(
                ctx,
                lowered,
                MaterializationReason::FunctionAbi,
            ))
        }

        // TypedArray `.length` can be shadowed by an own property, so use
        // the runtime length helper only when lowering has not already
        // registered the receiver as a native Buffer/TypedArray view above.
        Expr::PropertyGet {
            object, property, ..
        } if property == "length"
            && receiver_class_name(ctx, object)
                .as_deref()
                .is_some_and(is_numeric_typed_array_class) =>
        {
            let recv_box = lower_expr(ctx, object)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_value_length_f64", &[(DOUBLE, &recv_box)]))
        }

        // `arr.length` / `str.length` — INLINE. Both ArrayHeader and
        // StringHeader start with `length: u32` (`crates/perry-runtime/src
        // /array.rs` and `string.rs`). Same pattern: unbox pointer, load
        // u32 from offset 0, uitofp to double.
        // `.length` — INLINE for array, string, and interface-typed
        // receivers. Named types (interfaces, class instances) often
        // wrap strings or arrays at runtime, where length is at offset 0.
        Expr::PropertyGet {
            object, property, ..
        } if property == "length"
            && (is_array_expr(ctx, object)
                || is_string_expr(ctx, object)
                || match crate::type_analysis::static_type_of(ctx, object) {
                    // A `Function`-typed receiver is a closure, not a
                    // String/Array — its `.length` is the spec param
                    // count, served by the runtime reflection path
                    // (`closure_length` table). Loading a u32 from
                    // payload offset 0 here would read 0. Let it fall
                    // through to the generic property path.
                    Some(HirType::Named(n)) => n != "Function",
                    Some(HirType::Tuple(_)) => true,
                    _ => false,
                }) =>
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
            let tag_ok = blk.icmp_eq(I64, &recv_tag_masked, "32765"); // 0x7FFD
                                                                      // The tag check alone admits POINTER_TAG-boxed *handle-band*
                                                                      // values — Web Fetch handles (Headers/Request/Response/Blob, id
                                                                      // in [0x40000, 0xE0000)), net/http small handles, revocable-proxy
                                                                      // ids — which are NaN-boxed registry ids, NOT heap pointers. A
                                                                      // value statically typed Array/String/Named that actually holds
                                                                      // such a handle at runtime (e.g. a `Response`/`Headers` reaching a
                                                                      // `.length` site) would then `inttoptr` the bare id and load the
                                                                      // GC-type byte at `id-8` and the length u32 at `id` — both
                                                                      // unmapped low addresses → SIGSEGV (observed: doctor / mcp list
                                                                      // crashing at the exact fetch-handle address). The IC-miss path
                                                                      // (`js_object_get_field_ic_miss`) and the inline class-field guard
                                                                      // already gate on `> HANDLE_BAND_TOP`; mirror that here so any
                                                                      // handle-band receiver routes to the `js_value_length_f64` slow
                                                                      // path, which classifies it by registry without dereferencing the
                                                                      // raw id. `HANDLE_BAND_TOP` = 0xFFFFF (addr_class::HANDLE_BAND_MAX
                                                                      // - 1).
            let above_band = blk.icmp_ugt(I64, &recv_handle, "1048575"); // 0xFFFFF
            let handle_ok = blk.and(I1, &tag_ok, &above_band);
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
            let fast_len = ctx.block().uitofp(I32, &fast_len_i32, DOUBLE);
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
        Expr::PropertyGet {
            object, property, ..
        } if property == "size" && is_set_expr(ctx, object) => {
            let recv_box = lower_expr(ctx, object)?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let i32_v = blk.call(I32, "js_set_size", &[(I64, &recv_handle)]);
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        Expr::PropertyGet {
            object, property, ..
        } if property == "size" && is_map_expr(ctx, object) => {
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
        Expr::PropertyGet {
            object, property, ..
        } if property == "size" && is_url_search_params_expr(ctx, object) => {
            let recv_box = lower_expr(ctx, object)?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let i32_v = blk.call(I32, "js_url_search_params_size", &[(I64, &recv_handle)]);
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        // #6710: `class X extends URLSearchParams` instance `.size` — the
        // generic object-field lookup returns undefined (the entries live on the
        // hidden native backing, not a `size` field). `js_url_search_params_size`
        // resolves the backing internally, so pass the subclass instance.
        Expr::PropertyGet {
            object, property, ..
        } if property == "size" && is_url_search_params_subclass_expr(ctx, object) => {
            let recv_box = lower_expr(ctx, object)?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let i32_v = blk.call(I32, "js_url_search_params_size", &[(I64, &recv_handle)]);
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        Expr::PropertyGet {
            object,
            property,
            byte_offset,
        } => {
            if property == "prototype"
                && matches!(object.as_ref(), Expr::FuncRef(_) | Expr::Closure { .. })
            {
                let func_value = lower_expr(ctx, object)?;
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_function_prototype_value_for_read",
                    &[(DOUBLE, &func_value)],
                ));
            }
            if let Some((builtin_name, method_name)) =
                builtin_prototype_method_read(object, property)
            {
                let builtin_idx = ctx.strings.intern(builtin_name);
                let builtin_bytes_global =
                    format!("@{}", ctx.strings.entry(builtin_idx).bytes_global);
                let builtin_len = builtin_name.len().to_string();
                let method_idx = ctx.strings.intern(method_name);
                let method_bytes_global =
                    format!("@{}", ctx.strings.entry(method_idx).bytes_global);
                let method_len = method_name.len().to_string();
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_builtin_prototype_method_value",
                    &[
                        (PTR, &builtin_bytes_global),
                        (I64, &builtin_len),
                        (PTR, &method_bytes_global),
                        (I64, &method_len),
                    ],
                ));
            }
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
                // Devirt: register this module's runtime dispatch bucket before
                // the namespace value is produced, so later method calls on it
                // route to the real handlers. The CJS-`require` shim lowers
                // `require("path")` to `PropertyGet { NativeModuleRef("path"),
                // "default" }` (NOT a bare NativeModuleRef), so the bare-ref
                // install in `static_field_meta` never fired for the
                // require-then-`.default.join()` shape (Next.js' `_path.default
                // .join(...)` returned undefined — the dispatcher was unregistered
                // and `nm_dispatch_lookup` fell to the `None`/undefined arm).
                // Emitting it here mirrors the bare-ref path and keeps the
                // handlers alive against the auto-optimize dead-strip.
                if let Some(install_sym) = crate::nm_install::nm_install_symbol(module_name) {
                    ctx.block().call_void(install_sym, &[]);
                }
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
                // The value read of a native-module callable export (`const f =
                // util.inherits`) mints a BOUND_METHOD closure that, when invoked
                // indirectly, dispatches through the per-module `NM_DISPATCH_REGISTRY`
                // populated by `js_nm_install_<module>()`. The *direct* call form
                // (`util.inherits(a, b)`) is statically lowered to the runtime extern
                // and never touches the registry, so a module reached ONLY via this
                // value-read path would leave the registry empty and the indirect call
                // would resolve to `undefined` (winston/readable-stream's
                // `require('inherits')` → `util.inherits` value → `inherits(Sub, Base)`
                // silently skipped, breaking the ES5 super-chain). Emit the install
                // here so the value-read path's later dispatch finds the module fn.
                if let Some(install_sym) = crate::nm_install::nm_install_symbol(module_name) {
                    ctx.block().call_void(install_sym, &[]);
                }
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
                    let declared_raw_f64 = crate::type_analysis::scalar_replaced_field_is_raw_f64(
                        ctx,
                        object.as_ref(),
                        property,
                    );
                    let raw_f64_field =
                        crate::type_analysis::scalar_replaced_field_raw_f64_store_state(
                            ctx,
                            Some(*id),
                            property,
                            declared_raw_f64,
                        );
                    let lowered_js = LoweredValue {
                        semantic: SemanticKind::JsValue,
                        rep: NativeRep::JsValue,
                        llvm_ty: DOUBLE,
                        value: value.clone(),
                    };
                    ctx.record_lowered_value_with_access_mode(
                        "ScalarObjectFieldGet",
                        Some(*id),
                        "scalar_object_field_load",
                        &lowered_js,
                        None,
                        None,
                        None,
                        None,
                        false,
                        false,
                        vec![
                            format!("field={}", property),
                            format!("raw_f64_field={}", raw_f64_field as u8),
                        ],
                    );
                    if raw_f64_field {
                        let lowered_f64 = LoweredValue::f64(value.clone());
                        ctx.record_lowered_value_with_access_mode(
                            "ScalarObjectFieldGet",
                            Some(*id),
                            "scalar_object_field_load.raw_f64",
                            &lowered_f64,
                            None,
                            None,
                            None,
                            None,
                            false,
                            false,
                            vec![format!("field={}", property), "raw_f64_field=1".to_string()],
                        );
                    }
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
                if let Some(target_id) = ctx.scalar_ctor_target.last().copied() {
                    let slot = ctx
                        .scalar_replaced
                        .get(&target_id)
                        .and_then(|fs| fs.get(property.as_str()).cloned());
                    if let Some(slot) = slot {
                        let value = ctx.block().load(DOUBLE, &slot);
                        let declared_raw_f64 =
                            crate::type_analysis::scalar_replaced_field_is_raw_f64(
                                ctx,
                                object.as_ref(),
                                property,
                            );
                        let raw_f64_field =
                            crate::type_analysis::scalar_replaced_field_raw_f64_store_state(
                                ctx,
                                Some(target_id),
                                property,
                                declared_raw_f64,
                            );
                        let lowered_js = LoweredValue {
                            semantic: SemanticKind::JsValue,
                            rep: NativeRep::JsValue,
                            llvm_ty: DOUBLE,
                            value: value.clone(),
                        };
                        ctx.record_lowered_value_with_access_mode(
                            "ScalarThisFieldGet",
                            Some(target_id),
                            "scalar_object_field_load",
                            &lowered_js,
                            None,
                            None,
                            None,
                            None,
                            false,
                            false,
                            vec![
                                format!("field={}", property),
                                format!("raw_f64_field={}", raw_f64_field as u8),
                            ],
                        );
                        if raw_f64_field {
                            let lowered_f64 = LoweredValue::f64(value.clone());
                            ctx.record_lowered_value_with_access_mode(
                                "ScalarThisFieldGet",
                                Some(target_id),
                                "scalar_object_field_load.raw_f64",
                                &lowered_f64,
                                None,
                                None,
                                None,
                                None,
                                false,
                                false,
                                vec![format!("field={}", property), "raw_f64_field=1".to_string()],
                            );
                        }
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
            // arrives. Other unrecognized property shapes fall through
            // to the `undefined` sentinel (a spec-correct property miss).
            if matches!(object.as_ref(), Expr::GlobalGet(_)) {
                return lower_globalget_property(ctx, property);
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
                    // Missing namespace properties must return undefined,
                    // not the named-import fallback TAG_TRUE sentinel.
                    // Done before the class_ids check below because
                    // none of the recognized submodules export classes
                    // by name today; if/when they do (e.g.
                    // `readline/promises.Interface`), the class_ids
                    // branch still wins because class names get
                    // registered into both maps.
                    if let Some(submod_key) = ctx.namespace_node_submodules.get(name) {
                        let install_sym = crate::nm_install::nm_submod_install_symbol(submod_key);
                        let submod_label = emit_string_literal_global(ctx, submod_key);
                        let name_label = emit_string_literal_global(ctx, property);
                        let submod_len = submod_key.len();
                        let name_len = property.len();
                        let blk = ctx.block();
                        if let Some(s) = install_sym {
                            blk.call_void(s, &[]);
                        }
                        return Ok(blk.call(
                            DOUBLE,
                            "js_node_submodule_namespace_member",
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
                        // Issue #678/#5924: re-export renames mean the suffix
                        // in the origin module differs from the
                        // consumer-visible name. Namespace-scoped lookup
                        // first so a rename in a different namespace
                        // imported into this file can't clobber this
                        // namespace's unrenamed member of the same name.
                        let origin_suffix = import_origin_suffix_ns(
                            ctx.import_function_origin_names,
                            ctx.namespace_member_origin_names,
                            _ns_lookup_name.as_deref().unwrap_or(""),
                            property,
                        );
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
            //
            // Gate strictly on `imported_vars`: only exported const/let
            // bindings have a `perry_fn_<src>__<name>` *getter* whose call
            // returns the value. For an imported *function*, that same symbol
            // IS the function body — calling it here invoked the function with
            // zero args (reading garbage params) and read the property off its
            // return value. Stripe hit this on `StripeResource.method` /
            // `.extend` (an `export { StripeResource }` function with static
            // props); every static read invoked the constructor instead. The
            // function/class case falls through to the generic path below,
            // which materializes the closure value and reads its dynamic prop.
            if let Expr::ExternFuncRef { name, .. } = object.as_ref() {
                if ctx.imported_vars.contains(name) {
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
            }
            // Getter dispatch: if the receiver is a known class and
            // the property is registered as a getter, call the
            // synthesized __get_<property> method instead of doing a
            // raw field load.
            if let Some(class_name) = receiver_class_name(ctx, object) {
                if class_name == "URLPattern" && is_url_pattern_data_property(property) {
                    let recv_box = lower_expr(ctx, object)?;
                    let key_idx = ctx.strings.intern(property);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let blk = ctx.block();
                    let obj_bits = blk.bitcast_double_to_i64(&recv_box);
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
                // #6003: `class_name == "Headers"` only means the NATIVE
                // fetch Headers when the user hasn't defined their own
                // `class Headers` — a user class of that name owns the
                // receiver type, so fall through to the user-class
                // getter/method dispatch below.
                if class_name == "Headers"
                    && !ctx.classes.contains_key(&class_name)
                    && matches!(
                        property.as_str(),
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
                {
                    let recv_box = lower_expr(ctx, object)?;
                    let key_idx = ctx.strings.intern(property);
                    let entry = ctx.strings.entry(key_idx);
                    let bytes_global = format!("@{}", entry.bytes_global);
                    let len_str = entry.byte_len.to_string();
                    let blk = ctx.block();
                    let bytes_i64 = blk.ptrtoint(&bytes_global, I64);
                    return Ok(blk.call(
                        DOUBLE,
                        "js_headers_method_value",
                        &[(DOUBLE, &recv_box), (I64, &bytes_i64), (I64, &len_str)],
                    ));
                }
                if class_name == "ClientRequest" && is_http_client_request_method_name(property) {
                    return lower_class_method_bind(ctx, object, property);
                }
                if class_name == "Agent" && is_http_agent_method_name(property) {
                    return lower_class_method_bind(ctx, object, property);
                }
                if is_net_native_method_value(&class_name, property) {
                    return lower_class_method_bind(ctx, object, property);
                }
                if class_has_computed_runtime_members(ctx, &class_name) {
                    return lower_runtime_property_get_by_name(ctx, object, property);
                }
                let getter_key = (class_name.clone(), format!("__get_{}", property));
                // STATIC accessors are emitted with the static (no-`this`)
                // calling convention under a `perry_static_…` symbol, so the
                // instance direct-call ABI here would reference a symbol that
                // is never emitted (`__get_get_#f` undefined-value link error
                // for `static get #f()`). Route them through the dynamic
                // by-name dispatch below, which hits CLASS_STATIC_ACCESSORS.
                let is_static_accessor = ctx
                    .classes
                    .get(&class_name)
                    .map(|c| c.static_accessor_names.iter().any(|n| n == property))
                    .unwrap_or(false);
                if !is_static_accessor {
                    if let Some(fn_name) = ctx.methods.get(&getter_key).cloned() {
                        let recv_box = lower_expr(ctx, object)?;
                        return Ok(ctx.block().call(DOUBLE, &fn_name, &[(DOUBLE, &recv_box)]));
                    }
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
                        "getReader" | "cancel" | "tee" | "pipeTo" | "pipeThrough" | "values"
                    ) | (
                        "ReadableStreamDefaultReader",
                        "read" | "releaseLock" | "cancel"
                    ) | ("WritableStream", "getWriter" | "abort" | "close")
                        | (
                            "WritableStreamDefaultWriter",
                            "write" | "close" | "abort" | "releaseLock"
                        )
                );
                if class_name == "Headers"
                    && !ctx.classes.contains_key(&class_name)
                    && is_headers_method_name(property)
                {
                    let recv_box = lower_expr(ctx, object)?;
                    let key_idx = ctx.strings.intern(property);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let blk = ctx.block();
                    let obj_bits = blk.bitcast_double_to_i64(&recv_box);
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                    let key_handle = blk.and(I64, &key_bits, POINTER_MASK_I64);
                    return Ok(blk.call(
                        DOUBLE,
                        "js_object_get_field_by_name_f64",
                        &[(I64, &obj_bits), (I64, &key_handle)],
                    ));
                }
                if is_web_stream_method {
                    return lower_class_method_bind(ctx, object, property);
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
                        // #5093 loop versioning: inside the fast clone of a
                        // class-field versioned loop, a tracked field read on
                        // the proven receiver lowers to a bare slot load on
                        // the preheader-cached object pointer — no shape
                        // check, no guard call, no fallback (the preheader
                        // proved the shape once and the call-free clone keeps
                        // it true; see stmt/loops.rs).
                        let loop_fact_ptr = match object.as_ref() {
                            Expr::LocalGet(recv_id) => crate::expr::class_field_loop_fact_lookup(
                                &ctx.class_field_loop_facts,
                                *recv_id,
                                &class_name,
                                property,
                            )
                            .filter(|(_, loop_idx)| *loop_idx == field_index)
                            .map(|(fact, _)| fact.obj_ptr.clone()),
                            _ => None,
                        };
                        if let Some(obj_ptr) = loop_fact_ptr {
                            let field_idx_str = field_index.to_string();
                            let header_skip =
                                crate::target_layout::object_header_size_bytes(ctx.target_triple)
                                    .to_string();
                            let blk = ctx.block();
                            let fields_base = blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
                            let field_ptr = blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
                            let val = blk.load(DOUBLE, &field_ptr);
                            let fast = LoweredValue {
                                semantic: SemanticKind::JsNumber,
                                rep: NativeRep::F64,
                                llvm_ty: DOUBLE,
                                value: val.clone(),
                            };
                            ctx.record_lowered_value_with_access_mode_and_facts(
                                "ClassFieldGet",
                                None,
                                "class_field_get.loop_raw_f64_load",
                                &fast,
                                Some(BoundsState::Guarded {
                                    guard_id: "class_field_loop_preheader_check".to_string(),
                                }),
                                None,
                                Some(BufferAccessMode::CheckedNative),
                                None,
                                None,
                                None,
                                vec![raw_f64_layout_fact(
                                    None,
                                    "consumed",
                                    "class_field_loop_preheader_check",
                                    None,
                                )],
                                Vec::new(),
                                false,
                                false,
                                vec![
                                    format!("class={}", class_name),
                                    format!("field={}", property),
                                    format!("field_index={}", field_idx_str),
                                    "receiver_proof=loop_preheader_shape_check".to_string(),
                                    "field_layout=raw_f64_slot_array".to_string(),
                                    "loop_versioning=class_field_fast_clone".to_string(),
                                ],
                            );
                            return Ok(val);
                        }
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
                        // #5391 path 2: oversized modules full-outline the entire
                        // class-field-GET diamond (guard + fast load + fallback +
                        // phi) to a single `js_class_field_get_ic(...)` call that
                        // returns the field value. This shrinks large minified
                        // user functions enough for clang -O0 to compile them
                        // (the per-function compile time is superlinear in size).
                        // Mirrors the field-SET full-outline (#5334 lever B).
                        if crate::codegen::full_outline_ic_enabled() {
                            let (key_raw, expected_keys) = {
                                let blk = ctx.block();
                                let key_box = blk.load(DOUBLE, &key_handle_global);
                                let key_bits = blk.bitcast_double_to_i64(&key_box);
                                let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                                let expected_keys =
                                    blk.load(I64, &format!("@{}", keys_global_name));
                                (key_raw, expected_keys)
                            };
                            let val = ctx.block().call(
                                DOUBLE,
                                "js_class_field_get_ic",
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
                            return Ok(val);
                        }
                        // #5093: build the guard operands once, up front, so both
                        // the inline shape pre-check and the guard-call fallback
                        // can reference them.
                        let (obj_bits, obj_handle, key_raw, expected_keys) = {
                            let blk = ctx.block();
                            let obj_bits = blk.bitcast_double_to_i64(&recv_box);
                            let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                            let key_box = blk.load(DOUBLE, &key_handle_global);
                            let key_bits = blk.bitcast_double_to_i64(&key_box);
                            let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                            let expected_keys = blk.load(I64, &format!("@{}", keys_global_name));
                            (obj_bits, obj_handle, key_raw, expected_keys)
                        };
                        let fast_idx = ctx.new_block("class_field_get.fast");
                        let fallback_idx = ctx.new_block("class_field_get.fallback");
                        let merge_idx = ctx.new_block("class_field_get.merge");
                        let fast_label = ctx.block_label(fast_idx);
                        let fallback_label = ctx.block_label(fallback_idx);
                        let merge_label = ctx.block_label(merge_idx);

                        // #5093: inline shape pre-check. On a monomorphic hit it
                        // branches straight to the fast slot load, skipping the
                        // cross-crate guard call; on a miss it leaves the current
                        // block at the guard-call path below (unchanged).
                        let _guardcall_label =
                            crate::expr::class_field_inline_guard::emit_class_field_inline_precheck(
                                ctx,
                                &obj_bits,
                                &obj_handle,
                                &expected_class_id_str,
                                &expected_keys,
                                field_index,
                                requires_raw_f64,
                                None,
                                &fast_label,
                            );
                        let guard_ok = ctx.block().call(
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
                        let guard_pass = ctx.block().icmp_ne(I32, &guard_ok, "0");
                        ctx.block()
                            .cond_br(&guard_pass, &fast_label, &fallback_label);

                        ctx.current_block = fast_idx;
                        // arm64_32 watchOS: the object fields region begins at
                        // `size_of::<ObjectHeader>()` past the user pointer — 24 on
                        // 64-bit, 20 on ILP32 (the trailing `keys_array` pointer is 4
                        // bytes there). A hardcoded 24 reads every class field 4 bytes
                        // off on a 32-bit watch, so this inline class-field load
                        // disagreed with the generic-PIC load / runtime setter (both
                        // target-aware) and typed-object string fields came back as
                        // word-swapped NaN-boxes. Derive it from the target triple
                        // (no-op on 64-bit; see `target_layout`).
                        let header_skip =
                            crate::target_layout::object_header_size_bytes(ctx.target_triple)
                                .to_string();
                        let blk = ctx.block();
                        let obj_ptr = blk.inttoptr(I64, &obj_handle);
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
                                    format!("class_id={}", expected_class_id_str),
                                    format!("field={}", property),
                                    format!("field_index={}", field_idx_str),
                                    "receiver_proof=declared_named_receiver_guarded_exact_class"
                                        .to_string(),
                                    "field_layout=raw_f64_slot_array".to_string(),
                                    "pointer_bitmap=non_pointer".to_string(),
                                ],
                            );
                        }

                        ctx.current_block = fallback_idx;
                        let blk = ctx.block();
                        blk.call_void("js_typed_feedback_record_fallback_call", &[(I64, &site_id)]);
                        let val_fallback_js = blk.call(
                            DOUBLE,
                            "js_object_get_field_by_name_f64",
                            &[(I64, &obj_bits), (I64, &key_raw)],
                        );
                        let val_fallback = val_fallback_js.clone();
                        let fallback_end_label = blk.label.clone();
                        blk.br(&merge_label);
                        if requires_raw_f64 {
                            let fallback = LoweredValue {
                                semantic: SemanticKind::JsValue,
                                rep: NativeRep::JsValue,
                                llvm_ty: DOUBLE,
                                value: val_fallback_js.clone(),
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
                    return lower_class_method_bind(ctx, object, property);
                }
            }
            lower_generic_property_get(ctx, object, property, *byte_offset)
        }

        // -------- Ternary `cond ? a : b` (Phase B.7) --------
        // Lowered like if-expression with phi merge — same shape as the
        // logical operator path but with both branches always evaluated
        // conditionally on the truthiness test.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
