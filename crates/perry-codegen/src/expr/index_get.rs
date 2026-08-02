//! IndexGet (arr[i] / obj[k]).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::types::Type as HirType;
use perry_hir::{BinaryOp, Expr};

use crate::nanbox::POINTER_MASK_I64;
use crate::native_value::{
    BoundsState, BufferAccessMode, LoweredValue, MaterializationReason, NativeRep, SemanticKind,
};
use crate::type_analysis::{is_array_expr, is_numeric_expr, is_string_expr, receiver_class_name};
use crate::types::{DOUBLE, I1, I16, I32, I64, I8};

use super::{
    array_kind_fact, buffer_access_materialization_reason, emit_typed_feedback_register_site,
    expr_has_numeric_pointer_free_array_layout, int_range_expr, lower_buffer_load, lower_expr,
    lower_expr_as_i32, lower_typed_array_load, materialize_js_value, raw_f64_layout_fact,
    try_lower_flat_const_index_get, typed_feedback_emission_enabled, unbox_str_handle,
    unbox_to_i64, BufferAccessSpec, FnCtx, PackedF64LoopFact, TypedFeedbackContract,
    TypedFeedbackKind,
};

mod guarded_array;
mod inline_dyn_typed_array;

use guarded_array::{
    lower_guarded_array_index_get, lower_packed_f64_loop_index_get, packed_f64_loop_fact,
};
use inline_dyn_typed_array::lower_inline_dyn_typed_array_get;

fn is_width_tracked_typed_array_receiver(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    matches!(
        receiver_class_name(ctx, object).as_deref(),
        Some(
            "Int8Array"
                | "Uint8ClampedArray"
                | "Int16Array"
                | "Uint16Array"
                | "Int32Array"
                | "Uint32Array"
                | "Float16Array"
                | "Float32Array"
                | "Float64Array"
        )
    )
}

fn is_uint8array_receiver(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    matches!(
        receiver_class_name(ctx, object).as_deref(),
        Some("Buffer" | "Uint8Array")
    )
}

pub(crate) fn numeric_index_has_integer_array_index_proof(ctx: &FnCtx<'_>, index: &Expr) -> bool {
    fn range_is_nonnegative_i32(ctx: &FnCtx<'_>, index: &Expr) -> bool {
        int_range_expr(ctx, index)
            .is_some_and(|range| range.min >= 0 && range.max <= i32::MAX as i64)
    }

    match index {
        Expr::Integer(i) => (0..=i32::MAX as i64).contains(i),
        Expr::Number(n) => n.is_finite() && n.fract() == 0.0 && *n >= 0.0 && *n <= i32::MAX as f64,
        Expr::Binary { op, left, right } if matches!(op, BinaryOp::BitAnd) => {
            bitand_has_nonnegative_i32_mask(left, right)
        }
        Expr::LocalGet(id) => {
            ctx.integer_locals.contains(id)
                && ctx.i32_counter_slots.contains_key(id)
                && (ctx.nonnegative_integer_locals.contains(id)
                    || ctx
                        .int_range_facts
                        .iter()
                        .any(|fact| fact.local_id == *id && fact.range.min >= 0))
                || range_is_nonnegative_i32(ctx, index)
        }
        _ => range_is_nonnegative_i32(ctx, index),
    }
}

use super::proven_view_access::bitand_has_nonnegative_i32_mask;

/// #6011: decompose a packed-loop index expression into `(counter_local_id,
/// constant_offset)`. Matches `i`, `i + c`, `c + i`, and `i - c` with a small
/// |c| — exactly the shapes the packed-f64 range loop matcher admits, so any
/// offset seen here on a fact-carrying (array, counter) pair is inside the
/// range guard's validated window.
pub(crate) fn packed_f64_loop_index_parts(index: &Expr) -> Option<(u32, i32)> {
    use perry_hir::BinaryOp;
    match index {
        Expr::LocalGet(id) => Some((*id, 0)),
        Expr::Binary { op, left, right } if matches!(op, BinaryOp::Add | BinaryOp::Sub) => {
            let (id, offset) = match (left.as_ref(), right.as_ref()) {
                (Expr::LocalGet(id), Expr::Integer(c)) => {
                    let offset = if matches!(op, BinaryOp::Sub) {
                        c.checked_neg()?
                    } else {
                        *c
                    };
                    (*id, offset)
                }
                (Expr::Integer(c), Expr::LocalGet(id)) if matches!(op, BinaryOp::Add) => (*id, *c),
                _ => return None,
            };
            let offset = i32::try_from(offset).ok()?;
            if offset.unsigned_abs() > 64 {
                return None;
            }
            Some((id, offset))
        }
        _ => None,
    }
}

/// Look up a packed-f64 loop fact for `(arr, index-expr)`. Zero-offset
/// indices match any fact; non-zero offsets only match hole-tolerant facts
/// (established by the range guard, which validated the whole offset window —
/// the length-bound guard of the classic matcher only proves `i` itself).
fn packed_f64_loop_fact_for_index(
    ctx: &FnCtx<'_>,
    arr_id: u32,
    index: &Expr,
) -> Option<(PackedF64LoopFact, u32, i32)> {
    let (idx_id, offset) = packed_f64_loop_index_parts(index)?;
    let fact = packed_f64_loop_fact(ctx, arr_id, idx_id)?;
    if offset != 0 && !fact.allow_holes && !fact.window_validated {
        return None;
    }
    Some((fact, idx_id, offset))
}

/// Load the packed-loop counter's i32 shadow slot and apply the constant
/// index offset.
fn load_packed_loop_index_i32(ctx: &mut FnCtx<'_>, i32_slot: &str, offset: i32) -> String {
    let idx_i32 = ctx.block().load(I32, i32_slot);
    match offset.cmp(&0) {
        std::cmp::Ordering::Equal => idx_i32,
        std::cmp::Ordering::Greater => ctx.block().add(I32, &idx_i32, &offset.to_string()),
        std::cmp::Ordering::Less => ctx.block().sub(I32, &idx_i32, &(-offset).to_string()),
    }
}

fn numeric_index_has_loop_array_index_proof(ctx: &FnCtx<'_>, object: &Expr, index: &Expr) -> bool {
    let Expr::LocalGet(arr_id) = object else {
        return false;
    };
    let Some((idx_id, offset)) = packed_f64_loop_index_parts(index) else {
        return false;
    };
    if !ctx.i32_counter_slots.contains_key(&idx_id) {
        return false;
    }
    if packed_f64_loop_fact_for_index(ctx, *arr_id, index).is_some() {
        return true;
    }
    offset == 0
        && ctx
            .bounded_index_pairs
            .iter()
            .any(|fact| fact.array_local_id == *arr_id && fact.index_local_id == idx_id)
}

fn numeric_index_needs_runtime_key(ctx: &FnCtx<'_>, object: &Expr, index: &Expr) -> bool {
    // The inline array fast paths take an i32 index, so the conversion is only
    // sound after proving JS array-index semantics. A dynamic numeric value like
    // `let k = 1.5; arr[k]` must reach the runtime key helper and read the
    // property "1.5" instead of truncating to element 1.
    is_numeric_expr(ctx, index)
        && !numeric_index_has_integer_array_index_proof(ctx, index)
        && !numeric_index_has_loop_array_index_proof(ctx, object, index)
}

fn typed_array_index_needs_runtime_key(ctx: &FnCtx<'_>, object: &Expr, index: &Expr) -> bool {
    !numeric_index_has_integer_array_index_proof(ctx, index)
        && !numeric_index_has_loop_array_index_proof(ctx, object, index)
}

fn lower_array_index_get_via_runtime_key(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_double: &str,
    coerce_numeric_fallback: bool,
) -> String {
    let arr_handle = {
        let blk = ctx.block();
        unbox_to_i64(blk, arr_box)
    };
    let boxed = ctx.block().call(
        DOUBLE,
        "js_array_get_index_or_string",
        &[(I64, &arr_handle), (DOUBLE, idx_double)],
    );
    if coerce_numeric_fallback {
        ctx.block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &boxed)])
    } else {
        boxed
    }
}

fn is_async_dispose_symbol_index(index: &Expr) -> bool {
    let Expr::SymbolFor(symbol_name) = index else {
        return false;
    };
    match symbol_name.as_ref() {
        Expr::String(name) => name == "@@__perry_wk_asyncDispose",
        Expr::WtfString(name) => name.as_slice() == b"@@__perry_wk_asyncDispose",
        _ => false,
    }
}

/// True when `object` evaluates to an INT32-tagged class ref or class
/// prototype ref (NaN-box tag `0x7FFE`) rather than a heap-pointer object.
/// Such values must reach `js_object_get_field_by_name` with their tag bits
/// intact — masking with `POINTER_MASK_I64` strips the `0x7FFE` tag and the
/// runtime never routes to the class-static-accessor / prototype-vtable
/// lookup, so `(class { static get 0(){} })['0']` reads `undefined`.
///
/// Covers three shapes:
///   * `Expr::ClassRef` / imported-class `ExternFuncRef` — a class ref value.
///   * `LocalGet` of a local aliased to a class (`var C = class {…}` lowers to
///     `Let { init: ClassRef }`, recording `local_class_aliases["C"] = "C"`).
///     A literal member name (`C.x`) folds to `PropertyGet` and resolves on its
///     own, but an integer-like / empty key stays an `IndexGet` and lands here.
///   * `C.prototype` of either of the above — a prototype ref value, so
///     `C.prototype['']` reaches the instance-vtable getter.
pub(crate) fn index_object_is_class_or_proto_ref(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    match object {
        Expr::ClassRef(_) => true,
        Expr::ExternFuncRef { name, .. } => ctx.class_ids.contains_key(name),
        Expr::LocalGet(id) => ctx
            .local_id_to_name
            .get(id)
            .and_then(|name| ctx.local_class_aliases.get(name))
            .map(|cls| ctx.class_ids.contains_key(cls))
            .unwrap_or(false),
        Expr::PropertyGet {
            object: inner,
            property,
            ..
        } if property.as_str() == "prototype" => {
            index_object_is_class_or_proto_ref(ctx, inner.as_ref())
        }
        _ => false,
    }
}

/// Compute the receiver handle to pass to `js_object_get_field_by_name`-family
/// helpers from a NaN-boxed receiver value (`obj_bits`). Only a *genuine heap
/// pointer* — POINTER_TAG (`0x7FFD`, plain objects/arrays) or STRING_TAG
/// (`0x7FFF`, heap strings) — may be masked down to a raw pointer for the runtime
/// to dereference. Every other receiver shape keeps its full NaN-boxed bits:
///
/// * an INT32-tagged class ref (`0x7FFE`) keeps its tag so the runtime routes to
///   the static field / method / accessor tables (test262 class/elements
///   propertyHelper `isWritable(C, name)` does `C[name]`);
/// * a plain number, SSO string, bigint, or bool/null/undefined keeps its bits
///   so the by-name runtime helper recognizes the tag and returns `undefined`
///   instead of masking the value's low 48 bits into a bogus heap address and
///   dereferencing it.
///
/// The masking-everything-but-classref predecessor crashed on `(<number>)[k]`:
/// the timestamp float `dayjs(1749820051142)` (`0x4279_7696_70ec_6000`) had its
/// low 48 bits (`0x7696_70ec_6000`) masked into a plausible-looking heap pointer,
/// and `js_typed_feedback_object_get_field_by_name_f64` then deref'd `ptr - 8`
/// for the GcHeader → SIGSEGV (#5429). Keeping full bits routes the number
/// through `normalize_raw_object_addr`, which rejects it (top16 `0x4279` masks to
/// `0`), matching the dotted `n.format` path's receiver-tag triage.
///
/// When the receiver's heap-pointer-ness is known at compile time
/// (`static_known` — a class or `.prototype` ref), pass full bits unconditionally;
/// otherwise branch at runtime on the tag so a runtime class-ref value (e.g. a
/// function parameter bound to a class — `function f(C, k){ return C[k]; }`) is
/// handled too.
pub(crate) fn classref_preserving_handle(
    blk: &mut crate::block::LlBlock,
    obj_bits: &str,
    static_known: bool,
) -> String {
    if static_known {
        return obj_bits.to_string();
    }
    let top16 = blk.lshr(I64, obj_bits, "48");
    // (top16 & 0xFFFD) == 0x7FFD is true for exactly POINTER_TAG (0x7FFD) and
    // STRING_TAG (0x7FFF) — the two heap-pointer-carrying tags.
    let masked_tag = blk.and(I64, &top16, "65533"); // 0xFFFD
    let is_heap_ptr = blk.icmp_eq(I64, &masked_tag, "32765"); // 0x7FFD
    let masked = blk.and(I64, obj_bits, POINTER_MASK_I64);
    blk.select(crate::types::I1, &is_heap_ptr, I64, &masked, obj_bits)
}

fn lower_class_method_bind(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    method_name: &str,
) -> Result<String> {
    let recv_box = lower_expr(ctx, object)?;
    let key_idx = ctx.strings.intern(method_name);
    let entry = ctx.strings.entry(key_idx);
    let bytes_global = format!("@{}", entry.bytes_global);
    let len_str = entry.byte_len.to_string();
    let blk = ctx.block();
    let bytes_i64 = blk.ptrtoint(&bytes_global, I64);
    Ok(blk.call(
        DOUBLE,
        "js_class_method_bind",
        &[(DOUBLE, &recv_box), (I64, &bytes_i64), (I64, &len_str)],
    ))
}

// Callers always supply an index that is statically a non-negative `i32`
// (proven via `numeric_index_has_integer_array_index_proof` / a bounded loop
// counter), so the guard takes only `idx_i32` (no `f64` index) — keeping the
// int→fp conversion out of the hot region. The boxed fallback still needs the
// `f64` index, so it is materialized lazily inside the (cold) fallback block.
/// Repsel 4a.2 (#6904): the receiver's repairable local slot, when it is a
/// plain (non-boxed, non-captured) stack local. The guard tiers' COLD arm
/// stores the chain-followed live array head back into it so a stale
/// growth-forwarded binding (e.g. left behind by a specialized-ABI callee
/// growing a caller-allocated array) self-heals instead of pinning every
/// access to the boxed fallback. Boxed/captured locals are excluded — their
/// slot holds the box/capture pointer, not the array head.
fn receiver_repair_slot(ctx: &FnCtx<'_>, object: &Expr) -> Option<String> {
    let Expr::LocalGet(id) = object else {
        return None;
    };
    if ctx.boxed_vars.contains(id) || ctx.closure_captures.contains_key(id) {
        return None;
    }
    ctx.locals.get(id).cloned()
}

pub(crate) fn lower_numeric_index_get_for_number_context(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
) -> Result<Option<String>> {
    let Expr::IndexGet { object, index } = expr else {
        return Ok(None);
    };
    // Masked-window fast path first: the dense range guard proved the whole
    // static index window at loop entry, so the read needs neither the static
    // layout proof below nor a per-access guard. The fact can only exist for
    // a range-loop-eligible binding (never scalar-replaced or aliased).
    if let Expr::LocalGet(arr_id) = object.as_ref() {
        if let Some(fact) =
            super::masked_window::masked_window_fact_for_index(ctx, *arr_id, index.as_ref())
        {
            let arr_box = lower_expr(ctx, object)?;
            let idx_i32 = lower_expr_as_i32(ctx, index)?;
            return Ok(Some(super::masked_window::lower_masked_window_index_get(
                ctx, *arr_id, &arr_box, &idx_i32, &fact,
            )));
        }
    }
    if !is_array_expr(ctx, object) || !expr_has_numeric_pointer_free_array_layout(ctx, object) {
        return Ok(None);
    }

    // A scalar-replaced array has no heap allocation at all: its elements live
    // in stack slots and the local never holds an array. Lowering `arr[i]`
    // through the guarded element path would take that empty slot as the
    // receiver — the guard sees a null array and declines, and the boxed
    // fallback coerces the resulting `undefined` to NaN. So
    // `const a = [1, 2, 3]; a[0] + 1` produced NaN while a bare `a[0]` (which
    // `lower` serves from the scalar slot) was correct. Leave these locals to
    // `lower`, exactly as the inline-TA path already does.
    if let Expr::LocalGet(id) = object.as_ref() {
        if ctx.scalar_replaced_arrays.contains_key(id)
            || ctx.array_row_aliases.contains_key(id)
            || ctx.scalar_replaced.contains_key(id)
        {
            return Ok(None);
        }
    }

    // Repsel Phase 4a.3: guard-free `Ptr<NumArray>` load — supersedes the
    // packed/bounded/guarded tiers when the local proof + a per-site
    // in-bounds proof both hold.
    if let Some(value) = super::ptr_numarray_access::try_lower_num_array_guard_free_get(
        ctx,
        object.as_ref(),
        index.as_ref(),
    )? {
        return Ok(Some(value));
    }

    if let Expr::LocalGet(arr_id) = object.as_ref() {
        if let Some((fact, idx_id, offset)) =
            packed_f64_loop_fact_for_index(ctx, *arr_id, index.as_ref())
        {
            if let Some(i32_slot) = ctx.i32_counter_slots.get(&idx_id).cloned() {
                let arr_box = lower_expr(ctx, object)?;
                let idx_i32 = load_packed_loop_index_i32(ctx, &i32_slot, offset);
                return Ok(Some(lower_packed_f64_loop_index_get(
                    ctx, *arr_id, &arr_box, &idx_i32, &fact,
                )));
            }
        }
    }
    if let (Expr::LocalGet(arr_id), Expr::LocalGet(idx_id)) = (object.as_ref(), index.as_ref()) {
        if ctx
            .bounded_index_pairs
            .iter()
            .any(|fact| fact.index_local_id == *idx_id && fact.array_local_id == *arr_id)
        {
            if let Some(i32_slot) = ctx.i32_counter_slots.get(idx_id).cloned() {
                let repair_slot = receiver_repair_slot(ctx, object);
                let arr_box = lower_expr(ctx, object)?;
                let idx_i32 = ctx.block().load(I32, &i32_slot);
                return lower_guarded_array_index_get(
                    ctx,
                    &arr_box,
                    &idx_i32,
                    "bidx.num",
                    true,
                    true,
                    repair_slot.as_deref(),
                )
                .map(Some);
            }
        }
    }

    let repair_slot = receiver_repair_slot(ctx, object);
    let arr_box = lower_expr(ctx, object)?;
    if !numeric_index_has_integer_array_index_proof(ctx, index) {
        let idx_double = lower_expr(ctx, index)?;
        return Ok(Some(lower_array_index_get_via_runtime_key(
            ctx,
            &arr_box,
            &idx_double,
            true,
        )));
    }
    let idx_i32 = lower_expr_as_i32(ctx, index)?;
    lower_guarded_array_index_get(
        ctx,
        &arr_box,
        &idx_i32,
        "arr",
        true,
        true,
        repair_slot.as_deref(),
    )
    .map(Some)
}

/// #5525: lower an `S[i]` read whose receiver is an *untyped* (`any`/unknown)
/// local — bcryptjs's Blowfish `S`/`P`/`lr` boxes reach their `Int32Array`
/// state through plain `Array.<number>` parameters — directly as a guaranteed
/// **Number**, for use as a non-`+` arithmetic / bitwise operand.
///
/// The generic `obj[i]` lowering ([`lower_inline_dyn_typed_array_get`] via
/// [`lower`]) already emits the guarded inline typed-array load, but leaves its
/// cold slow branch boxed, so the arithmetic site must wrap the whole result in
/// a per-element `js_number_coerce`. Here we instead sink that coercion into the
/// slow branch (`coerce_slow_to_number = true`), so the hot per-kind fast path —
/// ~100% of bcrypt's ~600M reads — pays *no* coerce at all and the caller skips
/// its site coerce. Operators like `^`/`-`/`*`/`<<` always `ToNumber` their
/// operands, so coercing early is semantics-preserving; `+` (which may be string
/// concat) never reaches this path (its untyped operands route through
/// `js_dynamic_string_or_number_add`).
///
/// Returns `None` (caller falls back to `lower_expr` + a site coerce) unless the
/// receiver is exactly a non-special `any`/unknown `LocalGet` — the one shape
/// for which [`lower`]'s `IndexGet` arm provably reaches the inline-TA path, so
/// this never diverges from the value the generic path would have produced.
pub(crate) fn lower_unknown_local_index_get_for_number_context(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
) -> Result<Option<String>> {
    let Expr::IndexGet { object, index } = expr else {
        return Ok(None);
    };
    // Receiver must be a plain local of erased static type. Restricting to
    // `LocalGet` (not arbitrary expressions) guarantees none of `lower`'s
    // earlier `IndexGet` branches (Server/globalThis/width-tracked-TA/Uint8Array/
    // scalar-replaced/flat-const/class-ref/string-receiver) can pre-empt the
    // inline-TA path, so coercing the slow branch here matches the generic path.
    let Expr::LocalGet(id) = object.as_ref() else {
        return Ok(None);
    };
    // #6750 follow-up: an active masked-window fact wins over the guarded
    // inline-TA probe — the fact's entry guard already proved storage + the
    // whole index window, so the read needs no per-access cache probe at all.
    if let Some(fact) = super::masked_window::masked_window_fact_for_index(ctx, *id, index.as_ref())
    {
        let arr_box = lower_expr(ctx, object)?;
        let idx_i32 = lower_expr_as_i32(ctx, index)?;
        return Ok(Some(super::masked_window::lower_masked_window_index_get(
            ctx, *id, &arr_box, &idx_i32, &fact,
        )));
    }
    let recv_unknown = matches!(
        crate::type_analysis::static_type_of(ctx, object),
        None | Some(HirType::Any) | Some(HirType::Unknown)
    );
    if !recv_unknown {
        return Ok(None);
    }
    // Bail if this local is tracked by any specialized lowering that `lower`
    // would dispatch ahead of the inline-TA path.
    if ctx.scalar_replaced_arrays.contains_key(id)
        || ctx.array_row_aliases.contains_key(id)
        || ctx.scalar_replaced.contains_key(id)
        || is_string_expr(ctx, object)
        || index_object_is_class_or_proto_ref(ctx, object)
    {
        return Ok(None);
    }
    // A statically-string / symbol key is an ordinary [[Get]], not an element
    // read — leave it to `lower`'s dedicated routes.
    let index_is_static_string_or_symbol = matches!(
        index.as_ref(),
        Expr::String(_) | Expr::WtfString(_) | Expr::SymbolFor(_)
    ) || is_string_expr(ctx, index);
    if index_is_static_string_or_symbol {
        return Ok(None);
    }
    let obj_box = lower_expr(ctx, object)?;
    let idx_d = lower_expr(ctx, index)?;
    Ok(Some(lower_inline_dyn_typed_array_get(
        ctx, &obj_box, &idx_d, true,
    )))
}

fn lower_bounded_array_index_get(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
) -> Result<String> {
    let blk = ctx.block();
    let arr_bits = blk.bitcast_double_to_i64(arr_box);
    let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);

    // Issue #179 Phase 3: lazy-array guard on the bounded-index fast path.
    // Same story as the generic path below: a LazyArrayHeader has unrelated
    // bytes at `arr + 8 + idx*8`, so route through the slow path only when
    // the receiver is lazy. Issue #233: also detect FORWARDED arrays; the
    // slow path's `clean_arr_ptr` follows the chain.
    let gc_type_addr = blk.sub(I64, &arr_handle, "8");
    let gc_type_ptr = blk.inttoptr(I64, &gc_type_addr);
    let gc_type = blk.load(I8, &gc_type_ptr);
    let is_lazy = blk.icmp_eq(I8, &gc_type, "9"); // GC_TYPE_LAZY_ARRAY
    let gc_flags_addr = blk.sub(I64, &arr_handle, "7");
    let gc_flags_ptr = blk.inttoptr(I64, &gc_flags_addr);
    let gc_flags = blk.load(I8, &gc_flags_ptr);
    let fwd_bits = blk.and(I8, &gc_flags, "128"); // GC_FLAG_FORWARDED
    let is_fwd = blk.icmp_ne(I8, &fwd_bits, "0");
    let needs_slow = blk.or(I1, &is_lazy, &is_fwd);
    // Index accessors / custom attribute descriptors (`Object.defineProperty
    // (arr, i, { get })`) divert element reads through the descriptor tables —
    // the raw slot load below would bypass them (test262 sort/precise-*).
    // GcHeader._reserved (u16 at -6) carries OBJ_FLAG_ARRAY_DESCRIPTORS=0x400.
    let obj_flags_addr = blk.sub(I64, &arr_handle, "6");
    let obj_flags_ptr = blk.inttoptr(I64, &obj_flags_addr);
    let obj_flags = blk.load(I16, &obj_flags_ptr);
    let desc_bits = blk.and(I16, &obj_flags, "1024");
    let has_desc = blk.icmp_ne(I16, &desc_bits, "0");
    let needs_slow = blk.or(I1, &needs_slow, &has_desc);

    let lazy_idx = ctx.new_block("bidx.lazy");
    let fast_idx = ctx.new_block("bidx.fast");
    let merge_idx = ctx.new_block("bidx.merge");
    let lazy_label = ctx.block_label(lazy_idx);
    let fast_label = ctx.block_label(fast_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&needs_slow, &lazy_label, &fast_label);

    ctx.current_block = lazy_idx;
    let lazy_blk = ctx.block();
    let lazy_val = lazy_blk.call(
        DOUBLE,
        "js_array_get_f64",
        &[(I64, &arr_handle), (I32, idx_i32)],
    );
    let lazy_end_label = lazy_blk.label.clone();
    lazy_blk.br(&merge_label);

    ctx.current_block = fast_idx;
    let fast_blk = ctx.block();
    let idx_i64 = fast_blk.zext(I32, idx_i32, I64);
    let byte_offset = fast_blk.shl(I64, &idx_i64, "3");
    let with_header = fast_blk.add(I64, &byte_offset, "8");
    let element_addr = fast_blk.add(I64, &arr_handle, &with_header);
    let element_ptr = fast_blk.inttoptr(I64, &element_addr);
    let fast_raw = fast_blk.load(DOUBLE, &element_ptr);
    // `new Array(n)` slots are TAG_HOLE internally; JavaScript reads expose
    // `undefined`.
    let fast_raw_bits = fast_blk.bitcast_double_to_i64(&fast_raw);
    let is_hole = fast_blk.icmp_eq(I64, &fast_raw_bits, crate::nanbox::TAG_HOLE_I64);
    let undef_d = fast_blk.bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
    let fast_val = fast_blk.select(I1, &is_hole, DOUBLE, &undef_d, &fast_raw);
    let fast_end_label = fast_blk.label.clone();
    fast_blk.br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(
        DOUBLE,
        &[(&fast_val, &fast_end_label), (&lazy_val, &lazy_end_label)],
    ))
}

// #6132: retired — inline-read a value as a plain ArrayHeader, which is unsafe
// for off-heap typed arrays (garbage reads). Callers now use the typed-feedback
// guarded path. Kept temporarily behind allow(dead_code); slated for deletion.
#[allow(dead_code)]
fn lower_legacy_array_index_get(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
) -> Result<String> {
    let blk = ctx.block();
    let arr_bits = blk.bitcast_double_to_i64(arr_box);
    let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);

    // Lazy/forwarded arrays need the runtime helper because their payload is
    // not the ordinary ArrayHeader element layout. Plain arrays stay fully
    // inline, including the bounds check and HOLE -> undefined translation.
    let gc_type_addr = blk.sub(I64, &arr_handle, "8");
    let gc_type_ptr = blk.inttoptr(I64, &gc_type_addr);
    let gc_type = blk.load(I8, &gc_type_ptr);
    let is_lazy = blk.icmp_eq(I8, &gc_type, "9"); // GC_TYPE_LAZY_ARRAY
    let gc_flags_addr = blk.sub(I64, &arr_handle, "7");
    let gc_flags_ptr = blk.inttoptr(I64, &gc_flags_addr);
    let gc_flags = blk.load(I8, &gc_flags_ptr);
    let fwd_bits = blk.and(I8, &gc_flags, "128"); // GC_FLAG_FORWARDED
    let is_fwd = blk.icmp_ne(I8, &fwd_bits, "0");
    let needs_slow = blk.or(I1, &is_lazy, &is_fwd);
    // Index accessors / custom attribute descriptors (`Object.defineProperty
    // (arr, i, { get })`) divert element reads through the descriptor tables —
    // the raw slot load below would bypass them (test262 sort/precise-*).
    // GcHeader._reserved (u16 at -6) carries OBJ_FLAG_ARRAY_DESCRIPTORS=0x400.
    let obj_flags_addr = blk.sub(I64, &arr_handle, "6");
    let obj_flags_ptr = blk.inttoptr(I64, &obj_flags_addr);
    let obj_flags = blk.load(I16, &obj_flags_ptr);
    let desc_bits = blk.and(I16, &obj_flags, "1024");
    let has_desc = blk.icmp_ne(I16, &desc_bits, "0");
    let needs_slow = blk.or(I1, &needs_slow, &has_desc);

    let lazy_idx = ctx.new_block("arr.lazy");
    let fast_idx = ctx.new_block("arr.fast");
    let merge_idx = ctx.new_block("arr.merge");
    let lazy_label = ctx.block_label(lazy_idx);
    let fast_label = ctx.block_label(fast_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&needs_slow, &lazy_label, &fast_label);

    ctx.current_block = lazy_idx;
    let lazy_blk = ctx.block();
    let lazy_val = lazy_blk.call(
        DOUBLE,
        "js_array_get_f64",
        &[(I64, &arr_handle), (I32, idx_i32)],
    );
    let lazy_end_label = lazy_blk.label.clone();
    lazy_blk.br(&merge_label);

    ctx.current_block = fast_idx;
    let fast_blk = ctx.block();
    let len_i32 = fast_blk.safe_load_i32_from_ptr(&arr_handle);
    let in_bounds = fast_blk.icmp_ult(I32, idx_i32, &len_i32);
    let ok_idx = ctx.new_block("arr.ok");
    let oob_idx = ctx.new_block("arr.oob");
    let ok_label = ctx.block_label(ok_idx);
    let oob_label = ctx.block_label(oob_idx);
    ctx.block().cond_br(&in_bounds, &ok_label, &oob_label);

    ctx.current_block = ok_idx;
    let blk = ctx.block();
    let idx_i64 = blk.zext(I32, idx_i32, I64);
    let byte_offset = blk.shl(I64, &idx_i64, "3");
    let with_header = blk.add(I64, &byte_offset, "8");
    let element_addr = blk.add(I64, &arr_handle, &with_header);
    let element_ptr = blk.inttoptr(I64, &element_addr);
    let raw = blk.load(DOUBLE, &element_ptr);
    let raw_bits = blk.bitcast_double_to_i64(&raw);
    let is_hole = blk.icmp_eq(I64, &raw_bits, crate::nanbox::TAG_HOLE_I64);
    let undef_d = blk.bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
    let val = blk.select(I1, &is_hole, DOUBLE, &undef_d, &raw);
    let ok_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = oob_idx;
    let undef_bits = crate::nanbox::i64_literal(crate::nanbox::TAG_UNDEFINED);
    let undef_val = ctx.block().bitcast_i64_to_double(&undef_bits);
    let oob_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(
        DOUBLE,
        &[
            (&val, &ok_end_label),
            (&undef_val, &oob_end_label),
            (&lazy_val, &lazy_end_label),
        ],
    ))
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::IndexGet { object, index } => {
            if receiver_class_name(ctx, object).as_deref() == Some("Server")
                && is_async_dispose_symbol_index(index)
            {
                return lower_class_method_bind(ctx, object, "@@__perry_wk_asyncDispose");
            }
            // Issue #611: `globalThis[<key>]` reads from the persistent
            // global-this singleton. Pre-fix, `Expr::GlobalGet` lowered
            // to the `0.0` sentinel and the generic IndexGet path called
            // `js_object_get_field_by_name_f64(0, key)` which returned
            // undefined — `(globalThis as any)[id] = m; (globalThis as
            // any)[id]` round-trip lost the value. Route through the
            // real singleton (`js_get_global_this`) when receiver is
            // GlobalGet AND the key is string-typed.
            if matches!(object.as_ref(), Expr::GlobalGet(_))
                && (matches!(index.as_ref(), Expr::String(_)) || is_string_expr(ctx, index))
            {
                let key_box = lower_expr(ctx, index)?;
                let blk = ctx.block();
                let key_handle = unbox_str_handle(blk, &key_box);
                return Ok(blk.call(
                    DOUBLE,
                    "js_global_or_console_property_by_name",
                    &[(I64, &key_handle)],
                ));
            }
            if is_width_tracked_typed_array_receiver(ctx, object) {
                // A symbol-keyed read on a typed array (`ta[Symbol.toStringTag]`,
                // `ta[Symbol.iterator]`) must NOT take the dynamic-index helper
                // below — it stringifies the key and reads an ordinary [[Get]],
                // missing the `%TypedArray%.prototype` symbol accessors. Route
                // symbol keys to the symbol-property resolver (mirrors the array
                // path), which exposes `@@toStringTag` (`safe-stable-stringify`)
                // and `@@iterator`.
                if matches!(index.as_ref(), Expr::SymbolFor(_)) {
                    let obj_box = lower_expr(ctx, object)?;
                    let key_box = lower_expr(ctx, index)?;
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_object_get_symbol_property",
                        &[(DOUBLE, &obj_box), (DOUBLE, &key_box)],
                    ));
                }
                // #2063 / fractional numeric keys: only proven integer element
                // indices may take an i32 helper path. Try native
                // buffer-view lowering first because it carries stronger
                // bounds facts than the syntactic integer-key predicate.
                if let Some(value) = lower_typed_array_load(ctx, object, index)? {
                    return Ok(materialize_js_value(
                        ctx,
                        value,
                        MaterializationReason::RuntimeApi,
                    ));
                }
                // Phase 2 checked tier: see expr/proven_view_access.rs.
                if let Some(v) = super::try_lower_proven_view_checked_f64_load(ctx, object, index)?
                {
                    return Ok(v);
                }
                if typed_array_index_needs_runtime_key(ctx, object.as_ref(), index.as_ref()) {
                    let arr_box = lower_expr(ctx, object)?;
                    let key_box = lower_expr(ctx, index)?;
                    let blk = ctx.block();
                    let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                    let arr_i64 = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                    let result = blk.call(
                        DOUBLE,
                        "js_typed_array_index_get_dynamic",
                        &[(I64, &arr_i64), (DOUBLE, &key_box)],
                    );
                    let slow = LoweredValue::js_value(result.clone());
                    ctx.record_lowered_value_with_access_mode(
                        "TypedArrayGet",
                        None,
                        "TypedArrayGet.slow_path",
                        &slow,
                        Some(BoundsState::Unknown),
                        None,
                        Some(BufferAccessMode::DynamicFallback),
                        Some(buffer_access_materialization_reason(ctx, object)),
                        false,
                        false,
                        vec!["typed_array_fallback=untracked_or_unproven".to_string()],
                    );
                    return Ok(result);
                }

                // Numeric-context read of a typed-array PARAM (e.g. bcryptjs
                // `_encipher`'s `n = S[l >>> 24]; n += S[...]`): an inline checked
                // f64 load that is bit-exact with `js_typed_array_get` (numeric
                // element in-bounds, `TAG_UNDEFINED` OOB), replacing the per-read
                // runtime call. Gated on a proven integer index; guard misses
                // (view/detached/wrong-kind) defer to the memory-safe helper.
                if let Some(value) =
                    super::ta_param_f64_read::try_lower_ta_param_f64_read(ctx, object, index)?
                {
                    return Ok(value);
                }

                // Width-aware typed-array native lowering is only sound for
                // tracked fresh views with proven/guarded element bounds. All
                // aliases, reassigned locals, and unknown bounds stay on the
                // runtime helper, with artifact evidence for the fallback.
                let arr_box = lower_expr(ctx, object)?;
                let idx_double = lower_expr(ctx, index)?;
                let blk = ctx.block();
                let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                let arr_i64 = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                let idx_i32 = blk.fptosi(DOUBLE, &idx_double, I32);
                let result = blk.call(
                    DOUBLE,
                    "js_typed_array_get",
                    &[(I64, &arr_i64), (I32, &idx_i32)],
                );
                let slow = LoweredValue::js_value(result.clone());
                ctx.record_lowered_value_with_access_mode(
                    "TypedArrayGet",
                    None,
                    "TypedArrayGet.slow_path",
                    &slow,
                    Some(BoundsState::Unknown),
                    None,
                    Some(BufferAccessMode::DynamicFallback),
                    Some(buffer_access_materialization_reason(ctx, object)),
                    false,
                    false,
                    vec!["typed_array_fallback=untracked_or_unproven".to_string()],
                );
                return Ok(result);
            }
            if is_uint8array_receiver(ctx, object) && is_numeric_expr(ctx, index) {
                if let Some(value) =
                    lower_buffer_load(ctx, object, index, BufferAccessSpec::uint8array_get())?
                {
                    let reason = buffer_access_materialization_reason(ctx, object);
                    return Ok(materialize_js_value(ctx, value, reason));
                }
                if typed_array_index_needs_runtime_key(ctx, object.as_ref(), index.as_ref()) {
                    let arr_box = lower_expr(ctx, object)?;
                    let key_box = lower_expr(ctx, index)?;
                    let blk = ctx.block();
                    let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                    let arr_i64 = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                    return Ok(blk.call(
                        DOUBLE,
                        "js_typed_array_index_get_dynamic",
                        &[(I64, &arr_i64), (DOUBLE, &key_box)],
                    ));
                }
                // #6088: the index is a proven non-negative i32 key, but the
                // inline load above bailed, so its value is NOT proven in
                // bounds. The native `js_uint8array_get` accessor returns the
                // `0` byte-sentinel for an out-of-range read; a JS-value
                // `buf[i]` / `uint8array[i]` must instead read `undefined`
                // (ECMAScript IntegerIndexedExotic `[[Get]]`). Route the
                // unproven-bounds slow path through the JS-value getter (robust
                // for both a real Uint8Array and a Buffer-backed receiver) —
                // in-range reads still return the byte as a number.
                let arr_box = lower_expr(ctx, object)?;
                let idx_i32 = lower_expr_as_i32(ctx, index)?;
                let blk = ctx.block();
                let handle = unbox_to_i64(blk, &arr_box);
                return Ok(blk.call(
                    DOUBLE,
                    "js_uint8array_index_get_value",
                    &[(I64, &handle), (I32, &idx_i32)],
                ));
            }
            // Scalar-replaced array literal: `arr[k]` where arr was bound to
            // `[...]` and never escaped, and k is a compile-time index in
            // range. Loads directly from the kth stack alloca — no heap,
            // no runtime call, no bounds check. See `collect_non_escaping_arrays`.
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(slots) = ctx.scalar_replaced_arrays.get(id).cloned() {
                    let k = match index.as_ref() {
                        Expr::Integer(k) if *k >= 0 => Some(*k as usize),
                        Expr::Number(f) if f.is_finite() && *f >= 0.0 && f.fract() == 0.0 => {
                            Some(*f as usize)
                        }
                        _ => None,
                    };
                    if let Some(k) = k {
                        if k < slots.len() {
                            let value = ctx.block().load(DOUBLE, &slots[k]);
                            let raw_f64_element =
                                crate::type_analysis::scalar_replaced_array_element_is_raw_f64(
                                    ctx,
                                    object.as_ref(),
                                    index.as_ref(),
                                );
                            let lowered_js = LoweredValue {
                                semantic: SemanticKind::JsValue,
                                rep: NativeRep::JsValue,
                                llvm_ty: DOUBLE,
                                value: value.clone(),
                            };
                            ctx.record_lowered_value_with_access_mode(
                                "ScalarArrayIndexGet",
                                Some(*id),
                                "scalar_array_element_load",
                                &lowered_js,
                                None,
                                None,
                                None,
                                None,
                                false,
                                false,
                                vec![
                                    format!("index={}", k),
                                    format!("raw_f64_element={}", raw_f64_element as u8),
                                ],
                            );
                            if raw_f64_element {
                                let lowered_f64 = LoweredValue::f64(value.clone());
                                ctx.record_lowered_value_with_access_mode(
                                    "ScalarArrayIndexGet",
                                    Some(*id),
                                    "scalar_array_element_load.raw_f64",
                                    &lowered_f64,
                                    None,
                                    None,
                                    None,
                                    None,
                                    false,
                                    false,
                                    vec![format!("index={}", k), "raw_f64_element=1".to_string()],
                                );
                            }
                            return Ok(value);
                        }
                    }
                }
            }

            // Issue #50: flat-const 2D int array fast path. Replaces
            // `X[i][j]` (inline) and `krow[j]` (aliased row pattern)
            // with a direct GEP + load from a private `[N x i32]`
            // global emitted at module compile. Skips the arena header
            // + length check + double reload per access. Returns the
            // element as a NaN-boxed double (`sitofp i32 → double`) so
            // callers that expect fp receive the same JSValue shape
            // they already do; callers that expect i32 (via the #49
            // `lower_expr_as_i32` path) collapse the `fptosi(sitofp)`
            // round-trip during instcombine.
            if let Some(v) = try_lower_flat_const_index_get(ctx, object, index)? {
                return Ok(v);
            }

            // String indexing fast path: `s[i]` returns the char at
            // position i as a single-char string. Handled before the
            // array path so `str[0]` doesn't fall through to a raw
            // double load.
            if is_string_expr(ctx, object) {
                let s_box = lower_expr(ctx, object)?;
                let idx_d = lower_expr(ctx, index)?;
                let blk = ctx.block();
                // #3987: route through the canonical-index runtime helper (it
                // takes the raw NaN-boxed key, not an `fptosi`'d i32) so a valid
                // array index returns its char and every non-canonical key
                // (`NaN`, `1.5`, negatives, OOB, `"01"`, non-numeric strings)
                // returns `undefined` — matching ECMAScript / Node — instead of
                // truncating the index and returning `""` for OOB.
                // Pass the receiver STILL BOXED. Unboxing here masked off the
                // low 48 bits, which is only a pointer for a heap STRING_TAG
                // value — an inline SHORT_STRING_TAG (SSO) value's payload is
                // the characters themselves, so the mask produced a garbage
                // pointer and `(a + b)[0]` segfaulted on any short
                // concatenation. The boxed entry point decides by tag.
                return Ok(blk.call(
                    DOUBLE,
                    "js_string_index_get_boxed",
                    &[(DOUBLE, &s_box), (DOUBLE, &idx_d)],
                ));
            }
            // #6750 follow-up: a masked-window fact (dense range-loop or
            // straight-line region fast copy) covering this access means the
            // entry guard already proved the receiver's storage layout and
            // the whole static index window — the read is a bare inline load
            // even though the STATIC type is erased (`any` parameter). Must
            // run before the unknown-receiver `js_dyn_index_get` route below.
            if let Expr::LocalGet(arr_id) = object.as_ref() {
                if let Some(fact) =
                    super::masked_window::masked_window_fact_for_index(ctx, *arr_id, index.as_ref())
                {
                    let arr_box = lower_expr(ctx, object)?;
                    let idx_i32 = lower_expr_as_i32(ctx, index)?;
                    return Ok(super::masked_window::lower_masked_window_index_get(
                        ctx, *arr_id, &arr_box, &idx_i32, &fact,
                    ));
                }
            }
            // Issue #514: when the receiver's static type is genuinely
            // unknown (`Type::Any` / `Type::Unknown`) and the index is
            // numeric, route through the runtime tag-aware dispatcher.
            // The pre-fix array fast path interpreted `*StringHeader`
            // pointers as `*ArrayHeader`, returning the byte_len as a
            // subnormal f64 — the load-bearing bug behind hono's
            // mergePath template-literal logic that mixes `s?.[0]` /
            // `s?.at(-1)` / `s?.slice(1)` on `(s: any)` parameters.
            // The gate is narrow (only Type::Any/Unknown) so existing
            // TypedArray, Object-with-numeric-keys, and class-instance
            // fast paths keep their inline-offset reads.
            let recv_ty = crate::type_analysis::static_type_of(ctx, object);
            let recv_unknown = matches!(
                recv_ty,
                None | Some(perry_hir::types::Type::Any) | Some(perry_hir::types::Type::Unknown)
            );
            // #5525: route every non-static-string/symbol read on an unknown
            // receiver through `js_dyn_index_get` (numeric, runtime-string, and
            // runtime-symbol are all triaged in the runtime). The earlier
            // `is_numeric_expr(index)` gate missed `lr[off]`/`lr[off + 1]`
            // (bcryptjs `_encipher`'s `off` is an `any` param, so `off + 1` is
            // not provably numeric); statically-known string-literal / symbol
            // keys keep their dedicated interned-handle / symbol routes below.
            let index_is_static_string_or_symbol = matches!(
                index.as_ref(),
                Expr::String(_) | Expr::WtfString(_) | Expr::SymbolFor(_)
            ) || is_string_expr(ctx, index);
            if recv_unknown && !index_is_static_string_or_symbol {
                let obj_box = lower_expr(ctx, object)?;
                let idx_d = lower_expr(ctx, index)?;
                // #5525 follow-up: guarded inline typed-array element load at the
                // access site (cache probe + bounds check + direct slot load),
                // falling back to `js_dyn_index_get` on any guard miss. Removes
                // the per-element out-of-line call + `lookup_typed_array_kind` +
                // `js_number_coerce` on bcrypt's hot Int32Array `S[i]`/`P[i]`.
                return Ok(lower_inline_dyn_typed_array_get(
                    ctx, &obj_box, &idx_d, false,
                ));
            }
            // Three cases:
            //   1. Receiver is a known array → inline f64 element load
            //   2. Index is a string (literal or string-typed local) →
            //      generic object field access via js_object_get_field_by_name_f64
            //   3. Anything else → fall back to dynamic object field
            //      access by stringifying the index at runtime
            if is_array_expr(ctx, object) {
                // #321: a symbol-keyed array read (`arr[Symbol.iterator]`) must
                // NOT take the numeric fast path below — `fptosi` on the symbol
                // value yields a garbage index (returned a number). Route symbol
                // keys to the symbol-property resolver, which exposes the array
                // iterator for `Symbol.iterator`.
                if matches!(index.as_ref(), Expr::SymbolFor(_)) {
                    let obj_box = lower_expr(ctx, object)?;
                    let key_box = lower_expr(ctx, index)?;
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_object_get_symbol_property",
                        &[(DOUBLE, &obj_box), (DOUBLE, &key_box)],
                    ));
                }
                if !is_numeric_expr(ctx, index) {
                    let arr_box = lower_expr(ctx, object)?;
                    let idx_double = lower_expr(ctx, index)?;
                    let arr_handle = {
                        let blk = ctx.block();
                        unbox_to_i64(blk, &arr_box)
                    };
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_array_get_index_or_string",
                        &[(I64, &arr_handle), (DOUBLE, &idx_double)],
                    ));
                }
                if numeric_index_needs_runtime_key(ctx, object.as_ref(), index.as_ref()) {
                    let arr_box = lower_expr(ctx, object)?;
                    let idx_double = lower_expr(ctx, index)?;
                    let arr_handle = {
                        let blk = ctx.block();
                        unbox_to_i64(blk, &arr_box)
                    };
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_array_get_index_or_string",
                        &[(I64, &arr_handle), (DOUBLE, &idx_double)],
                    ));
                }
                let require_numeric_layout =
                    expr_has_numeric_pointer_free_array_layout(ctx, object);
                // Bounded-index fast path (mirrors the IndexSet
                // optimization in the same file): if the surrounding
                // for-loop registered `(counter_id, arr_id)` as
                // bounded via `lower_for`'s `classify_for_length_hoist`,
                // we can skip the bound check + OOB phi entirely.
                // The loop already proved `i < arr.length` and the
                // body provably can't change `arr.length`.
                if let Expr::LocalGet(arr_id) = object.as_ref() {
                    if let Some((fact, idx_id, offset)) =
                        packed_f64_loop_fact_for_index(ctx, *arr_id, index.as_ref())
                    {
                        if let Some(i32_slot) = ctx.i32_counter_slots.get(&idx_id).cloned() {
                            let arr_box = lower_expr(ctx, object)?;
                            let idx_i32 = load_packed_loop_index_i32(ctx, &i32_slot, offset);
                            return Ok(lower_packed_f64_loop_index_get(
                                ctx, *arr_id, &arr_box, &idx_i32, &fact,
                            ));
                        }
                    }
                    if let Some(fact) = super::masked_window::masked_window_fact_for_index(
                        ctx,
                        *arr_id,
                        index.as_ref(),
                    ) {
                        let arr_box = lower_expr(ctx, object)?;
                        let idx_i32 = lower_expr_as_i32(ctx, index)?;
                        return Ok(super::masked_window::lower_masked_window_index_get(
                            ctx, *arr_id, &arr_box, &idx_i32, &fact,
                        ));
                    }
                }
                if let (Expr::LocalGet(arr_id), Expr::LocalGet(idx_id)) =
                    (object.as_ref(), index.as_ref())
                {
                    if ctx.bounded_index_pairs.iter().any(|fact| {
                        fact.index_local_id == *idx_id && fact.array_local_id == *arr_id
                    }) {
                        if let Some(i32_slot) = ctx.i32_counter_slots.get(idx_id).cloned() {
                            let repair_slot = receiver_repair_slot(ctx, object);
                            let arr_box = lower_expr(ctx, object)?;
                            let idx_i32 = ctx.block().load(I32, &i32_slot);
                            if require_numeric_layout {
                                return lower_guarded_array_index_get(
                                    ctx,
                                    &arr_box,
                                    &idx_i32,
                                    "bidx.num",
                                    true,
                                    false,
                                    repair_slot.as_deref(),
                                );
                            }
                            return lower_bounded_array_index_get(ctx, &arr_box, &idx_i32);
                        }
                    }
                }

                let repair_slot = receiver_repair_slot(ctx, object);
                let arr_box = lower_expr(ctx, object)?;
                if !numeric_index_has_integer_array_index_proof(ctx, index) {
                    let idx_double = lower_expr(ctx, index)?;
                    return Ok(lower_array_index_get_via_runtime_key(
                        ctx,
                        &arr_box,
                        &idx_double,
                        false,
                    ));
                }
                let idx_i32 = lower_expr_as_i32(ctx, index)?;
                // #6132: a member receiver of unknown type (e.g. `n.buf[i]` where
                // `n.buf` is a Uint32Array) must NOT go through the legacy inline
                // reader — that path reads the value as a plain `ArrayHeader`
                // (gc_type at `handle-8`, raw f64 slot at `handle+8+i*8`), but a
                // small typed array is off-heap with no GcHeader, so both reads
                // are garbage and it returned nondeterministic junk. Route through
                // the typed-feedback-guarded path: its runtime guard rejects
                // non-plain arrays and takes the boxed fallback (which dispatches
                // typed arrays correctly), while plain arrays keep the fast path.
                return lower_guarded_array_index_get(
                    ctx,
                    &arr_box,
                    &idx_i32,
                    "arr",
                    require_numeric_layout,
                    false,
                    repair_slot.as_deref(),
                );
            }
            // Generic dynamic object access: stringify the index (no-op
            // for already-string keys, format for numeric keys) and
            // call js_object_get_field_by_name_f64.
            if let Expr::String(literal) = index.as_ref() {
                // Static string key: use the interned StringPool entry
                // so we get the same handle as obj["foo"].
                let preserve_class_ref_bits =
                    index_object_is_class_or_proto_ref(ctx, object.as_ref());
                let obj_box = lower_expr(ctx, object)?;
                let key_idx = ctx.strings.intern(literal);
                let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let blk = ctx.block();
                let obj_bits = blk.bitcast_double_to_i64(&obj_box);
                let obj_handle =
                    classref_preserving_handle(blk, &obj_bits, preserve_class_ref_bits);
                let key_box = blk.load(DOUBLE, &key_handle_global);
                let key_bits = blk.bitcast_double_to_i64(&key_box);
                let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                let site_id = emit_typed_feedback_register_site(
                    ctx,
                    TypedFeedbackKind::PropertyGet,
                    literal,
                    TypedFeedbackContract::object_get_by_name(),
                );
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_typed_feedback_object_get_field_by_name_f64",
                    &[(I64, &site_id), (I64, &obj_handle), (I64, &key_raw)],
                ));
            }
            if is_string_expr(ctx, index) {
                // Dynamic string key: unbox both pointers and call.
                // `key_handle` routes through `unbox_str_handle` because the
                // key may be an SSO value (e.g. from JSON.parse, .slice, or
                // any short-string-producing op); the runtime fn dereferences
                // it as `*StringHeader`. Issue #214 SSO bug class.
                let preserve_class_ref_bits =
                    index_object_is_class_or_proto_ref(ctx, object.as_ref());
                let obj_box = lower_expr(ctx, object)?;
                // #7154: `o[f()]` evaluates the base first and the key second,
                // leaving the base in a bare SSA register while `f()` runs. An
                // evacuating minor inside `f()` relocates the base; the location
                // it was read from is a root and gets rewritten, the register is
                // not, and the field read then dereferences from-space memory.
                //
                // This is the READ counterpart of #7192's `index_set` /
                // `property_set` receiver guard. zod's `core/checks.ts:68`
                // (`numericOriginMap[typeof def.value]`) is the instance that
                // SIGSEGV'd `sfw-registry --help` under
                // `PERRY_GC_MOVING_LOOP_POLLS=1`: `numericOriginMap` is a module
                // global (a registered root the collector rewrites) and the key
                // `typeof def.value` is a property get that can collect.
                let recv_guard =
                    super::temp_root::guard_store_operand(ctx, object, &obj_box, index);
                let key_box = lower_expr(ctx, index)?;
                let obj_box =
                    super::temp_root::reread_store_operand(ctx, &recv_guard, object, &obj_box)?;
                let blk = ctx.block();
                let obj_bits = blk.bitcast_double_to_i64(&obj_box);
                let obj_handle =
                    classref_preserving_handle(blk, &obj_bits, preserve_class_ref_bits);
                let key_handle = unbox_str_handle(blk, &key_box);
                let site_id = emit_typed_feedback_register_site(
                    ctx,
                    TypedFeedbackKind::PropertyGet,
                    "object[index]",
                    TypedFeedbackContract::object_get_by_name(),
                );
                let out = ctx.block().call(
                    DOUBLE,
                    "js_typed_feedback_object_get_field_by_name_f64",
                    &[(I64, &site_id), (I64, &obj_handle), (I64, &key_handle)],
                );
                super::temp_root::release_store_operand(ctx, recv_guard);
                return Ok(out);
            }
            // Last-resort fallback with runtime tag checks on the index.
            // First runtime-check whether the index is a Symbol; if so,
            // dispatch to the symbol-property side table — mirrors the
            // IndexSet branch. Otherwise fall through to string/numeric.
            let preserve_class_ref_bits = index_object_is_class_or_proto_ref(ctx, object.as_ref());
            let obj_box = lower_expr(ctx, object)?;
            // #7154: same window as the dynamic-string-key arm above — the base
            // is live in a register while the key expression is lowered, and an
            // evacuating minor inside the key relocates it.
            let recv_guard = super::temp_root::guard_store_operand(ctx, object, &obj_box, index);
            let idx_box = lower_expr(ctx, index)?;
            let obj_box =
                super::temp_root::reread_store_operand(ctx, &recv_guard, object, &obj_box)?;
            // RequireObjectCoercible(base): `null[k]` / `undefined[k]` must throw
            // a TypeError per spec, NOT silently return undefined. The dotted
            // PropertyGet path already guards nullish receivers; the computed
            // form fell through to the by-name runtime helper (masked handle
            // `2`/`1`) which returned undefined. The check fires here — after
            // both the base and the property-key *expression* are evaluated but
            // before ToPropertyKey (key coercion / `toString`) — matching the
            // ECMAScript evaluation order (test262 compound-assignment S11.13.2_A7.*,
            // prefix/postfix increment A6). A non-nullish receiver passes through
            // unchanged. (#4918 non-class language remnant.)
            let obj_box =
                ctx.block()
                    .call(DOUBLE, "js_require_object_coercible", &[(DOUBLE, &obj_box)]);
            let blk = ctx.block();
            let obj_bits = blk.bitcast_double_to_i64(&obj_box);
            let obj_handle = classref_preserving_handle(blk, &obj_bits, preserve_class_ref_bits);
            let is_sym_i32 = blk.call(I32, "js_is_symbol", &[(DOUBLE, &idx_box)]);
            let is_sym_bit = blk.icmp_ne(I32, &is_sym_i32, "0");
            let sym_idx = ctx.new_block("iget.sym");
            let nonsym_idx = ctx.new_block("iget.nonsym");
            let str_idx = ctx.new_block("iget.str");
            let num_idx = ctx.new_block("iget.num");
            let merge_idx = ctx.new_block("iget.merge");
            let sym_lbl = ctx.block_label(sym_idx);
            let nonsym_lbl = ctx.block_label(nonsym_idx);
            let str_lbl = ctx.block_label(str_idx);
            let num_lbl = ctx.block_label(num_idx);
            let merge_lbl = ctx.block_label(merge_idx);
            ctx.block().cond_br(&is_sym_bit, &sym_lbl, &nonsym_lbl);
            // Symbol key → side-table get.
            ctx.current_block = sym_idx;
            let v_sym = ctx.block().call(
                DOUBLE,
                "js_object_get_symbol_property",
                &[(DOUBLE, &obj_box), (DOUBLE, &idx_box)],
            );
            let sym_end_lbl = ctx.block().label.clone();
            ctx.block().br(&merge_lbl);
            // Not a symbol → recompute idx_bits in this block.
            ctx.current_block = nonsym_idx;
            let blk = ctx.block();
            let idx_bits = blk.bitcast_double_to_i64(&idx_box);
            let top16 = blk.lshr(I64, &idx_bits, "48");
            // STRING_TAG (0x7FFF = 32767): heap StringHeader pointer.
            let is_str_tag_heap = blk.icmp_eq(I64, &top16, "32767");
            let lower48 = blk.and(I64, &idx_bits, POINTER_MASK_I64);
            let is_valid_ptr = blk.icmp_ugt(I64, &lower48, "4095");
            let is_str_heap = blk.and(crate::types::I1, &is_str_tag_heap, &is_valid_ptr);
            // SHORT_STRING_TAG (0x7FF9 = 32761): inline SSO from JSON.parse,
            // .slice, etc. Lower 48 encode length+bytes, NOT a pointer, so we
            // can't AND-mask to a StringHeader; route through unbox_str_handle
            // which materializes SSO to a heap StringHeader (issue #434).
            let is_str_tag_sso = blk.icmp_eq(I64, &top16, "32761");
            let is_str = blk.or(crate::types::I1, &is_str_heap, &is_str_tag_sso);
            ctx.block().cond_br(&is_str, &str_lbl, &num_lbl);
            // String key → object field access.
            ctx.current_block = str_idx;
            let key_handle = {
                let blk = ctx.block();
                unbox_str_handle(blk, &idx_box)
            };
            let site_id = emit_typed_feedback_register_site(
                ctx,
                TypedFeedbackKind::PropertyGet,
                "object[index]",
                TypedFeedbackContract::object_get_by_name(),
            );
            let v_str = ctx.block().call(
                DOUBLE,
                "js_typed_feedback_object_get_field_by_name_f64",
                &[(I64, &site_id), (I64, &obj_handle), (I64, &key_handle)],
            );
            let str_end_lbl = ctx.block().label.clone();
            ctx.block().br(&merge_lbl);
            // Numeric key → polymorphic dispatch.
            //
            // Closes #471 (read side, paired with the IndexSet polymorphic
            // fix above): the previous fallback emitted an inline
            // `obj_handle + 8 + idx*8` load on the assumption that the
            // receiver had an ArrayHeader (8-byte header) layout. Once the
            // IndexSet path stopped writing through that layout for plain
            // objects, the read side had to follow — `constMap[i] = v;
            // constMap[i]` would otherwise set via the object setter
            // (key stringified into the keys_array) and read from
            // `obj+8+i*8` (stale ObjectHeader fields), returning garbage
            // f64 values.
            //
            // Route through the runtime which checks the receiver's GC
            // type and dispatches: arrays/lazy/buffers/typed-arrays
            // through js_array_get_f64 (handles forwarding-chain follow
            // + lazy-materialize + per-kind reads), plain objects
            // through stringify-the-index + js_object_get_field_by_name_f64.
            ctx.current_block = num_idx;
            let v_num = ctx.block().call(
                DOUBLE,
                "js_object_get_index_polymorphic",
                &[(I64, &obj_handle), (DOUBLE, &idx_box)],
            );
            let num_end_lbl = ctx.block().label.clone();
            ctx.block().br(&merge_lbl);
            // Merge.
            ctx.current_block = merge_idx;
            let merged = ctx.block().phi(
                DOUBLE,
                &[
                    (&v_sym, &sym_end_lbl),
                    (&v_str, &str_end_lbl),
                    (&v_num, &num_end_lbl),
                ],
            );
            // Released in the merge block so every arm's getter (any of which
            // can run a user getter and therefore collect) is still covered.
            super::temp_root::release_store_operand(ctx, recv_guard);
            Ok(merged)
        }

        // Phase H err: `agg.errors.length` — receiver is
        // PropertyGet(.., "errors") which resolves to a NaN-boxed
        // ArrayHeader pointer (via the dedicated "errors" arm below).
        // Inline-read length at offset 0 just like any other array.
        // Placed ahead of the generic length fast path so we don't
        // need static type analysis to recognize the shape.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
