//! ObjectRest..FsAppendFileSync (BigInt + Set methods + filesystem writers).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, Result};
use perry_hir::{BinaryOp, Expr};
use perry_types::Type as HirType;

use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::type_analysis::{
    is_bigint_expr, is_definitely_string_expr, is_numeric_expr, set_static_type_args,
};
use crate::types::{DOUBLE, F32, I1, I32, I64, PTR};

use super::{
    can_lower_expr_as_i32, i32_bool_to_nanbox, lower_expr, lower_expr_native, nanbox_bigint_inline,
    nanbox_pointer_inline, record_collection_number_key_fallback,
    record_collection_number_key_selected, record_collection_string_key_fallback,
    record_collection_string_key_selected, record_collection_typed_value_fallback,
    record_collection_typed_value_selected, unbox_to_i64, FnCtx,
};

fn number_coerce_operand_is_already_primitive_number(ctx: &FnCtx<'_>, operand: &Expr) -> bool {
    if crate::type_analysis::expr_may_return_boxed_value_from_raw_f64_fallback(ctx, operand)
        || is_bigint_expr(ctx, operand)
    {
        return false;
    }
    match operand {
        Expr::Integer(_)
        | Expr::Number(_)
        | Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. }
        | Expr::DateNow
        | Expr::Uint8ArrayLength(_)
        | Expr::BufferLength(_) => true,
        Expr::LocalGet(id) | Expr::Update { id, .. } => ctx.integer_locals.contains(id),
        Expr::Binary { op, left, right } => match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                number_coerce_operand_is_already_primitive_number(ctx, left)
                    && number_coerce_operand_is_already_primitive_number(ctx, right)
            }
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr
            | BinaryOp::UShr => true,
            BinaryOp::Pow => false,
        },
        _ => false,
    }
}

fn is_static_string_set(ctx: &FnCtx<'_>, set: &Expr) -> bool {
    matches!(
        set_static_type_args(ctx, set),
        Some([HirType::String | HirType::StringLiteral(_)])
    )
}

fn is_static_number_set(ctx: &FnCtx<'_>, set: &Expr) -> bool {
    matches!(set_static_type_args(ctx, set), Some([HirType::Number]))
}

fn guarded_set_number_add(ctx: &mut FnCtx<'_>, set_handle: &str, value_box: &str) -> String {
    let guard_raw = ctx
        .block()
        .call(I32, "js_typed_f64_arg_guard", &[(DOUBLE, value_box)]);
    let guard = ctx.block().icmp_ne(I32, &guard_raw, "0");
    let fast_idx = ctx.new_block("set_number.add.fast");
    let fallback_idx = ctx.new_block("set_number.add.fallback");
    let merge_idx = ctx.new_block("set_number.add.merge");
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&guard, &fast_label, &fallback_label);

    ctx.current_block = fast_idx;
    let value_raw = ctx
        .block()
        .call(DOUBLE, "js_typed_f64_arg_to_raw", &[(DOUBLE, value_box)]);
    let fast_value = ctx.block().call(
        I64,
        "js_set_add_number",
        &[(I64, set_handle), (DOUBLE, &value_raw)],
    );
    record_collection_number_key_selected(
        ctx,
        "SetAdd",
        "collection_number_value.set_add",
        &value_raw,
        "set",
        "number_value_helper",
        "js_set_add_number",
        "value",
    );
    let after_fast = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = fallback_idx;
    let fallback_value =
        ctx.block()
            .call(I64, "js_set_add", &[(I64, set_handle), (DOUBLE, value_box)]);
    record_collection_number_key_fallback(
        ctx,
        "SetAdd",
        "collection_number_value.set_add_generic",
        value_box,
        "set",
        "number_value_helper",
        "js_set_add",
        "runtime_value_guard_failed",
        "value",
    );
    let after_fallback = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    ctx.block().phi(
        I64,
        &[
            (fast_value.as_str(), after_fast.as_str()),
            (fallback_value.as_str(), after_fallback.as_str()),
        ],
    )
}

fn guarded_set_number_has(ctx: &mut FnCtx<'_>, set_handle: &str, value_box: &str) -> String {
    let guard_raw = ctx
        .block()
        .call(I32, "js_typed_f64_arg_guard", &[(DOUBLE, value_box)]);
    let guard = ctx.block().icmp_ne(I32, &guard_raw, "0");
    let fast_idx = ctx.new_block("set_number.has.fast");
    let fallback_idx = ctx.new_block("set_number.has.fallback");
    let merge_idx = ctx.new_block("set_number.has.merge");
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&guard, &fast_label, &fallback_label);

    ctx.current_block = fast_idx;
    let value_raw = ctx
        .block()
        .call(DOUBLE, "js_typed_f64_arg_to_raw", &[(DOUBLE, value_box)]);
    let fast_value = ctx.block().call(
        I32,
        "js_set_has_number",
        &[(I64, set_handle), (DOUBLE, &value_raw)],
    );
    record_collection_number_key_selected(
        ctx,
        "SetHas",
        "collection_number_value.set_has",
        &value_raw,
        "set",
        "number_value_helper",
        "js_set_has_number",
        "value",
    );
    let after_fast = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = fallback_idx;
    let fallback_value =
        ctx.block()
            .call(I32, "js_set_has", &[(I64, set_handle), (DOUBLE, value_box)]);
    record_collection_number_key_fallback(
        ctx,
        "SetHas",
        "collection_number_value.set_has_generic",
        value_box,
        "set",
        "number_value_helper",
        "js_set_has",
        "runtime_value_guard_failed",
        "value",
    );
    let after_fallback = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    ctx.block().phi(
        I32,
        &[
            (fast_value.as_str(), after_fast.as_str()),
            (fallback_value.as_str(), after_fallback.as_str()),
        ],
    )
}

fn guarded_set_number_delete(ctx: &mut FnCtx<'_>, set_handle: &str, value_box: &str) -> String {
    let guard_raw = ctx
        .block()
        .call(I32, "js_typed_f64_arg_guard", &[(DOUBLE, value_box)]);
    let guard = ctx.block().icmp_ne(I32, &guard_raw, "0");
    let fast_idx = ctx.new_block("set_number.delete.fast");
    let fallback_idx = ctx.new_block("set_number.delete.fallback");
    let merge_idx = ctx.new_block("set_number.delete.merge");
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&guard, &fast_label, &fallback_label);

    ctx.current_block = fast_idx;
    let value_raw = ctx
        .block()
        .call(DOUBLE, "js_typed_f64_arg_to_raw", &[(DOUBLE, value_box)]);
    let fast_value = ctx.block().call(
        I32,
        "js_set_delete_number",
        &[(I64, set_handle), (DOUBLE, &value_raw)],
    );
    record_collection_number_key_selected(
        ctx,
        "SetDelete",
        "collection_number_value.set_delete",
        &value_raw,
        "set",
        "number_value_helper",
        "js_set_delete_number",
        "value",
    );
    let after_fast = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = fallback_idx;
    let fallback_value = ctx.block().call(
        I32,
        "js_set_delete",
        &[(I64, set_handle), (DOUBLE, value_box)],
    );
    record_collection_number_key_fallback(
        ctx,
        "SetDelete",
        "collection_number_value.set_delete_generic",
        value_box,
        "set",
        "number_value_helper",
        "js_set_delete",
        "runtime_value_guard_failed",
        "value",
    );
    let after_fallback = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    ctx.block().phi(
        I32,
        &[
            (fast_value.as_str(), after_fast.as_str()),
            (fallback_value.as_str(), after_fallback.as_str()),
        ],
    )
}

fn is_static_i32_set(ctx: &FnCtx<'_>, set: &Expr) -> bool {
    matches!(set_static_type_args(ctx, set), Some([HirType::Int32]))
}

fn is_perry_u32_type(ctx: &FnCtx<'_>, ty: &HirType) -> bool {
    match ty {
        HirType::Named(name) if name == "PerryU32" => true,
        HirType::Named(name) => ctx
            .type_aliases
            .get(name)
            .is_some_and(|alias| is_perry_u32_type(ctx, alias)),
        _ => false,
    }
}

fn is_static_u32_set(ctx: &FnCtx<'_>, set: &Expr) -> bool {
    match set_static_type_args(ctx, set) {
        Some([value_ty]) => is_perry_u32_type(ctx, value_ty),
        _ => false,
    }
}

fn is_perry_f32_type(ctx: &FnCtx<'_>, ty: &HirType) -> bool {
    match ty {
        HirType::Named(name) if name == "PerryF32" => true,
        HirType::Named(name) => ctx
            .type_aliases
            .get(name)
            .is_some_and(|alias| is_perry_f32_type(ctx, alias)),
        _ => false,
    }
}

fn is_static_f32_set(ctx: &FnCtx<'_>, set: &Expr) -> bool {
    match set_static_type_args(ctx, set) {
        Some([value_ty]) => is_perry_f32_type(ctx, value_ty),
        _ => false,
    }
}

fn is_static_boolean_set(ctx: &FnCtx<'_>, set: &Expr) -> bool {
    matches!(set_static_type_args(ctx, set), Some([HirType::Boolean]))
}

fn can_lower_i32_for_collection_value(ctx: &FnCtx<'_>, value: &Expr) -> bool {
    can_lower_expr_as_i32(
        value,
        &ctx.i32_counter_slots,
        ctx.flat_const_arrays,
        &ctx.array_row_aliases,
        ctx.integer_locals,
        ctx.clamp3_functions,
        ctx.clamp_u8_functions,
        ctx.integer_returning_functions,
        ctx.i32_identity_functions,
    )
}

fn can_lower_u32_for_collection_value(ctx: &FnCtx<'_>, value: &Expr) -> bool {
    match value {
        Expr::Integer(n) => *n >= 0 && u32::try_from(*n).is_ok(),
        Expr::Binary {
            op: BinaryOp::UShr,
            left,
            right,
        } => {
            can_lower_i32_for_collection_value(ctx, left)
                && can_lower_i32_for_collection_value(ctx, right)
        }
        Expr::Uint8ArrayGet { .. }
        | Expr::BufferIndexGet { .. }
        | Expr::Uint8ArrayLength(_)
        | Expr::BufferLength(_) => true,
        Expr::LocalGet(id) => ctx.unsigned_i32_locals.contains(id),
        _ => false,
    }
}

fn literal_f64(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Integer(n) => Some(*n as f64),
        Expr::Number(n) => Some(*n),
        _ => None,
    }
}

fn f32_roundtrips_exact(value: f64) -> bool {
    let narrowed = value as f32;
    (narrowed as f64).to_bits() == value.to_bits()
}

fn can_lower_f32_for_collection_value(value: &Expr) -> bool {
    literal_f64(value).is_some_and(f32_roundtrips_exact)
}

fn can_lower_i1_for_collection_value(ctx: &FnCtx<'_>, value: &Expr) -> bool {
    match value {
        Expr::Bool(_) => true,
        Expr::LocalGet(id) => {
            ctx.i1_local_slots.contains_key(id)
                && !ctx.closure_captures.contains_key(id)
                && !ctx.boxed_vars.contains(id)
                && !ctx.module_globals.contains_key(id)
        }
        _ => false,
    }
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::ObjectRest {
            object,
            exclude_keys,
        } => {
            let obj_box = lower_expr(ctx, object)?;
            let key_handle_globals: Vec<String> = exclude_keys
                .iter()
                .map(|k| {
                    let idx = ctx.strings.intern(k);
                    format!("@{}", ctx.strings.entry(idx).handle_global)
                })
                .collect();
            let blk = ctx.block();
            let obj_handle = {
                let bits = blk.bitcast_double_to_i64(&obj_box);
                blk.and(I64, &bits, POINTER_MASK_I64)
            };
            let n_str = (exclude_keys.len() as u32).to_string();
            let keys_arr = blk.call(I64, "js_array_alloc_with_length", &[(I32, &n_str)]);
            for (i, handle_global) in key_handle_globals.iter().enumerate() {
                let idx_str = i.to_string();
                let key_box = blk.load(DOUBLE, handle_global);
                blk.call_void(
                    "js_array_set_f64_unchecked",
                    &[(I64, &keys_arr), (I32, &idx_str), (DOUBLE, &key_box)],
                );
            }
            let rest_ptr = blk.call(
                I64,
                "js_object_rest",
                &[(I64, &obj_handle), (I64, &keys_arr)],
            );
            Ok(nanbox_pointer_inline(blk, &rest_ptr))
        }

        // -------- BigInt(literal) --------
        // The HIR carries the literal as a string for arbitrary
        // precision. We hand it to the runtime as a UTF-8 byte
        // pointer + length.
        //
        // Tagged with BIGINT_TAG (not POINTER_TAG): `typeof 5n`
        // reads the top 16 bits to distinguish `"bigint"` from
        // `"object"`, and `js_dynamic_add`/`_sub`/`_mul`/`_div`/`_mod`
        // use `JSValue::is_bigint()` which also checks that tag —
        // literals tagged as POINTER_TAG fooled both sites, which is
        // why arithmetic used to collapse to `NaN`. Closes GH #33.
        Expr::BigInt(s) => {
            let bytes_idx = ctx.strings.intern(s);
            let bytes_global = format!("@{}", ctx.strings.entry(bytes_idx).bytes_global);
            let len_str = (s.len() as u32).to_string();
            let blk = ctx.block();
            let result = blk.call(
                I64,
                "js_bigint_from_string",
                &[(PTR, &bytes_global), (I32, &len_str)],
            );
            Ok(nanbox_bigint_inline(blk, &result))
        }

        // -------- BigInt(value) coercion --------
        // `BigInt(42)`, `BigInt("9223372036854775807")`, `BigInt(someBigInt)`.
        // The runtime helper inspects the NaN-box tag and dispatches:
        // bigint → pass-through, int32 → i64 conversion, string →
        // parse, undefined/null → 0n, f64 → truncate-to-i64. Result
        // is a raw `BigIntHeader*`; we NaN-box with BIGINT_TAG so
        // later sites see `typeof === "bigint"` and the dynamic-
        // arithmetic check `is_bigint()` both succeed.
        Expr::BigIntCoerce(operand) => {
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let ptr = blk.call(I64, "js_bigint_from_f64", &[(DOUBLE, &v)]);
            Ok(nanbox_bigint_inline(blk, &ptr))
        }

        // -------- arr.sort(comparator) -> same array (in place) --------
        // The HIR variant always carries a comparator. If the comparator
        // is a synthetic "default" marker we'd want js_array_sort_default;
        // for now we always use the user-comparator path.
        Expr::ArraySort { array, comparator } => {
            let arr_box = lower_expr(ctx, array)?;
            let cmp_box = lower_expr(ctx, comparator)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            // #2796: validate the comparator (function | undefined) before
            // sorting — throws TypeError for any other value, returns 0
            // (default sort) for undefined.
            let cmp_handle = blk.call(I64, "js_validate_array_comparator", &[(DOUBLE, &cmp_box)]);
            let result = blk.call(
                I64,
                "js_array_sort_with_comparator",
                &[(I64, &arr_handle), (I64, &cmp_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- arr.reduce(callback, initial?) -> value --------
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        }
        | Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let (has_init, init_d) = if let Some(init_expr) = initial {
                let v = lower_expr(ctx, init_expr)?;
                ("1".to_string(), v)
            } else {
                ("0".to_string(), "0x7FF8000000000000".to_string()) // NaN bits won't actually be used
            };
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            // Convert literal NaN bits to a double via bitcast — but the
            // string above isn't valid LLVM. Use a real NaN literal instead.
            let init_use = if has_init == "1" {
                init_d
            } else {
                // LLVM treats `0x7FF8000000000000` as a hex double literal
                // when written as `0x7FF8000000000000` — but the safe way
                // is to just use `0x7FF8000000000000` via the IR's hex
                // form for doubles. Use plain `0.0` since it's unused.
                "0.0".to_string()
            };
            let runtime_fn = if matches!(expr, Expr::ArrayReduceRight { .. }) {
                "js_array_reduce_right"
            } else {
                "js_array_reduce"
            };
            Ok(blk.call(
                DOUBLE,
                runtime_fn,
                &[
                    (I64, &arr_handle),
                    (I64, &cb_handle),
                    (I32, &has_init),
                    (DOUBLE, &init_use),
                ],
            ))
        }

        // -------- enum members lower to constants --------
        Expr::EnumMember {
            enum_name,
            member_name,
        } => {
            let key = (enum_name.clone(), member_name.clone());
            let val = ctx.enums.get(&key).ok_or_else(|| {
                anyhow!(
                    "perry-codegen: enum member {}.{} not found in enums table",
                    enum_name,
                    member_name
                )
            })?;
            match val {
                perry_hir::EnumValue::Number(n) => Ok(double_literal(*n as f64)),
                perry_hir::EnumValue::String(s) => {
                    // Intern the string and load the handle global at the
                    // use site, just like a regular string literal.
                    let key_idx = ctx.strings.intern(s);
                    let handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    Ok(ctx.block().load(DOUBLE, &handle_global))
                }
            }
        }

        // -------- fs.existsSync(path) -> boolean --------
        Expr::FsExistsSync(path) => {
            let p = lower_expr(ctx, path)?;
            let blk = ctx.block();
            let i32_v = blk.call(I32, "js_fs_exists_sync", &[(DOUBLE, &p)]);
            Ok(i32_bool_to_nanbox(blk, &i32_v))
        }

        // -------- Number(value) coercion --------
        Expr::NumberCoerce(operand) => {
            let already_number = number_coerce_operand_is_already_primitive_number(ctx, operand);
            let v = lower_expr(ctx, operand)?;
            if already_number {
                Ok(v)
            } else {
                Ok(ctx
                    .block()
                    .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &v)]))
            }
        }

        // -------- set.add(value) — updates the local in place --------
        Expr::SetAdd { set_id, value } => {
            let set_expr = Expr::LocalGet(*set_id);
            let receiver_i32_set = is_static_i32_set(ctx, &set_expr);
            let use_i32_set = receiver_i32_set && can_lower_i32_for_collection_value(ctx, value);
            let receiver_u32_set = is_static_u32_set(ctx, &set_expr);
            let use_u32_set = receiver_u32_set && can_lower_u32_for_collection_value(ctx, value);
            let receiver_f32_set = is_static_f32_set(ctx, &set_expr);
            let use_f32_set = receiver_f32_set && can_lower_f32_for_collection_value(value);
            let receiver_boolean_set = is_static_boolean_set(ctx, &set_expr);
            let use_boolean_set =
                receiver_boolean_set && can_lower_i1_for_collection_value(ctx, value);
            let receiver_number_set = is_static_number_set(ctx, &set_expr);
            let use_number_set = receiver_number_set && is_numeric_expr(ctx, value);
            let receiver_string_set = is_static_string_set(ctx, &set_expr);
            let value_is_string = is_definitely_string_expr(ctx, value);
            let use_string_set = receiver_string_set && value_is_string;
            let new_handle = if use_i32_set {
                let value_i32 =
                    lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::I32)?;
                let set_box = lower_expr(ctx, &set_expr)?;
                let set_handle = {
                    let blk = ctx.block();
                    unbox_to_i64(blk, &set_box)
                };
                let new_handle = {
                    let blk = ctx.block();
                    blk.call(
                        I64,
                        "js_set_add_i32",
                        &[(I64, &set_handle), (I32, &value_i32.value)],
                    )
                };
                record_collection_typed_value_selected(
                    ctx,
                    "SetAdd",
                    "collection_typed_value.set_add_i32",
                    &value_i32,
                    "set",
                    "int32_value_helper",
                    "js_set_add_i32",
                    "set_slot",
                );
                new_handle
            } else if use_u32_set {
                let value_u32 =
                    lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::U32)?;
                let set_box = lower_expr(ctx, &set_expr)?;
                let set_handle = {
                    let blk = ctx.block();
                    unbox_to_i64(blk, &set_box)
                };
                let new_handle = {
                    let blk = ctx.block();
                    blk.call(
                        I64,
                        "js_set_add_u32",
                        &[(I64, &set_handle), (I32, &value_u32.value)],
                    )
                };
                record_collection_typed_value_selected(
                    ctx,
                    "SetAdd",
                    "collection_typed_value.set_add_u32",
                    &value_u32,
                    "set",
                    "uint32_value_helper",
                    "js_set_add_u32",
                    "set_slot",
                );
                new_handle
            } else if use_f32_set {
                let value_f32 =
                    lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::F32)?;
                let set_box = lower_expr(ctx, &set_expr)?;
                let set_handle = {
                    let blk = ctx.block();
                    unbox_to_i64(blk, &set_box)
                };
                let new_handle = {
                    let blk = ctx.block();
                    blk.call(
                        I64,
                        "js_set_add_f32",
                        &[(I64, &set_handle), (F32, &value_f32.value)],
                    )
                };
                record_collection_typed_value_selected(
                    ctx,
                    "SetAdd",
                    "collection_typed_value.set_add_f32",
                    &value_f32,
                    "set",
                    "float32_value_helper",
                    "js_set_add_f32",
                    "set_slot",
                );
                new_handle
            } else if use_boolean_set {
                let value_i1 =
                    lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::I1)?;
                let set_box = lower_expr(ctx, &set_expr)?;
                let set_handle = {
                    let blk = ctx.block();
                    unbox_to_i64(blk, &set_box)
                };
                let new_handle = {
                    let blk = ctx.block();
                    let value_i32 = blk.zext(I1, &value_i1.value, I32);
                    blk.call(
                        I64,
                        "js_set_add_bool",
                        &[(I64, &set_handle), (I32, &value_i32)],
                    )
                };
                record_collection_typed_value_selected(
                    ctx,
                    "SetAdd",
                    "collection_typed_value.set_add_bool",
                    &value_i1,
                    "set",
                    "boolean_value_helper",
                    "js_set_add_bool",
                    "set_slot",
                );
                new_handle
            } else if use_number_set {
                let v = lower_expr(ctx, value)?;
                let set_box = lower_expr(ctx, &set_expr)?;
                let set_handle = {
                    let blk = ctx.block();
                    unbox_to_i64(blk, &set_box)
                };
                guarded_set_number_add(ctx, &set_handle, &v)
            } else {
                let set_box = lower_expr(ctx, &set_expr)?;
                let set_handle = {
                    let blk = ctx.block();
                    unbox_to_i64(blk, &set_box)
                };
                if use_string_set {
                    let value_ref = lower_expr_native(
                        ctx,
                        value,
                        crate::native_value::ExpectedNativeRep::StringRef,
                    )?;
                    let new_handle = {
                        let blk = ctx.block();
                        let new_handle = blk.call(
                            I64,
                            "js_set_add_string",
                            &[(I64, &set_handle), (I64, &value_ref.value)],
                        );
                        new_handle
                    };
                    record_collection_string_key_selected(
                        ctx,
                        "SetAdd",
                        "collection_string_key.set_add",
                        &value_ref.value,
                        "set",
                        "js_set_add_string",
                    );
                    record_collection_typed_value_selected(
                        ctx,
                        "SetAdd",
                        "collection_typed_value.set_add_string",
                        &value_ref,
                        "set",
                        "string_value_helper",
                        "js_set_add_string",
                        "set_slot",
                    );
                    new_handle
                } else {
                    let v = lower_expr(ctx, value)?;
                    let new_handle = {
                        let blk = ctx.block();
                        blk.call(I64, "js_set_add", &[(I64, &set_handle), (DOUBLE, &v)])
                    };
                    let reason = if receiver_string_set {
                        "value_expr_not_definitely_string"
                    } else {
                        "receiver_value_not_static_string"
                    };
                    if receiver_i32_set {
                        record_collection_typed_value_fallback(
                            ctx,
                            "SetAdd",
                            "collection_typed_value.set_add_generic",
                            &v,
                            "set",
                            "int32_value_helper",
                            "js_set_add",
                            "value_expr_not_native_i32",
                        );
                    } else if receiver_u32_set {
                        record_collection_typed_value_fallback(
                            ctx,
                            "SetAdd",
                            "collection_typed_value.set_add_generic",
                            &v,
                            "set",
                            "uint32_value_helper",
                            "js_set_add",
                            "value_expr_not_native_u32",
                        );
                    } else if receiver_f32_set {
                        record_collection_typed_value_fallback(
                            ctx,
                            "SetAdd",
                            "collection_typed_value.set_add_generic",
                            &v,
                            "set",
                            "float32_value_helper",
                            "js_set_add",
                            "value_expr_not_native_f32",
                        );
                    } else if receiver_boolean_set {
                        record_collection_typed_value_fallback(
                            ctx,
                            "SetAdd",
                            "collection_typed_value.set_add_generic",
                            &v,
                            "set",
                            "boolean_value_helper",
                            "js_set_add",
                            "value_expr_not_native_i1",
                        );
                    } else if receiver_number_set {
                        record_collection_number_key_fallback(
                            ctx,
                            "SetAdd",
                            "collection_number_value.set_add_generic",
                            &v,
                            "set",
                            "number_value_helper",
                            "js_set_add",
                            "value_expr_not_numeric",
                            "value",
                        );
                    } else {
                        record_collection_string_key_fallback(
                            ctx,
                            "SetAdd",
                            "collection_string_key.set_add_generic",
                            &v,
                            "set",
                            "js_set_add",
                            reason,
                        );
                    }
                    new_handle
                }
            };
            let blk = ctx.block();
            // `js_set_add*` mutate the set in place and ALWAYS return the same
            // `SetHeader` pointer they were given — `ensure_capacity` reallocs
            // only the internal elements buffer, never the header. So there is
            // no "realloc'd pointer" to write back: the previous writeback to
            // `set_id`'s storage was vestigial (copied from the array-push
            // pattern) and actively WRONG for a boxed/mutable closure capture —
            // it overwrote the capture SLOT (which holds a box pointer) with the
            // Set value, so the next read dereferenced the Set-as-box and saw
            // `undefined` (Next.js turbopack runtime: `loadedChunks.add(p)`
            // silently cleared the module-level `loadedChunks` Set captured by
            // the chunk-loader closure, SIGSEGV on the next `.add`). GC moves of
            // the header are handled by root rewriting of the variable slot, not
            // here. (origin/main bugfix; preserved over the scalar fast-path
            // restructure that produces `new_handle` above.)
            Ok(nanbox_pointer_inline(blk, &new_handle))
        }

        // -------- set.has(value) -> boolean --------
        Expr::SetHas { set, value } => {
            let receiver_i32_set = is_static_i32_set(ctx, set);
            let use_i32_set = receiver_i32_set && can_lower_i32_for_collection_value(ctx, value);
            let receiver_u32_set = is_static_u32_set(ctx, set);
            let use_u32_set = receiver_u32_set && can_lower_u32_for_collection_value(ctx, value);
            let receiver_f32_set = is_static_f32_set(ctx, set);
            let use_f32_set = receiver_f32_set && can_lower_f32_for_collection_value(value);
            let receiver_boolean_set = is_static_boolean_set(ctx, set);
            let use_boolean_set =
                receiver_boolean_set && can_lower_i1_for_collection_value(ctx, value);
            let receiver_number_set = is_static_number_set(ctx, set);
            let use_number_set = receiver_number_set && is_numeric_expr(ctx, value);
            let use_string_set =
                is_static_string_set(ctx, set) && is_definitely_string_expr(ctx, value);
            let s_box = lower_expr(ctx, set)?;
            let s_handle = {
                let blk = ctx.block();
                unbox_to_i64(blk, &s_box)
            };
            let i32_v = if use_i32_set {
                let value_i32 =
                    lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::I32)?;
                let i32_v = {
                    let blk = ctx.block();
                    blk.call(
                        I32,
                        "js_set_has_i32",
                        &[(I64, &s_handle), (I32, &value_i32.value)],
                    )
                };
                record_collection_typed_value_selected(
                    ctx,
                    "SetHas",
                    "collection_typed_value.set_has_i32",
                    &value_i32,
                    "set",
                    "int32_value_helper",
                    "js_set_has_i32",
                    "set_slot",
                );
                i32_v
            } else if use_u32_set {
                let value_u32 =
                    lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::U32)?;
                let i32_v = {
                    let blk = ctx.block();
                    blk.call(
                        I32,
                        "js_set_has_u32",
                        &[(I64, &s_handle), (I32, &value_u32.value)],
                    )
                };
                record_collection_typed_value_selected(
                    ctx,
                    "SetHas",
                    "collection_typed_value.set_has_u32",
                    &value_u32,
                    "set",
                    "uint32_value_helper",
                    "js_set_has_u32",
                    "set_slot",
                );
                i32_v
            } else if use_f32_set {
                let value_f32 =
                    lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::F32)?;
                let i32_v = {
                    let blk = ctx.block();
                    blk.call(
                        I32,
                        "js_set_has_f32",
                        &[(I64, &s_handle), (F32, &value_f32.value)],
                    )
                };
                record_collection_typed_value_selected(
                    ctx,
                    "SetHas",
                    "collection_typed_value.set_has_f32",
                    &value_f32,
                    "set",
                    "float32_value_helper",
                    "js_set_has_f32",
                    "set_slot",
                );
                i32_v
            } else if use_boolean_set {
                let value_i1 =
                    lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::I1)?;
                let i32_v = {
                    let blk = ctx.block();
                    let value_i32 = blk.zext(I1, &value_i1.value, I32);
                    blk.call(
                        I32,
                        "js_set_has_bool",
                        &[(I64, &s_handle), (I32, &value_i32)],
                    )
                };
                record_collection_typed_value_selected(
                    ctx,
                    "SetHas",
                    "collection_typed_value.set_has_bool",
                    &value_i1,
                    "set",
                    "boolean_value_helper",
                    "js_set_has_bool",
                    "set_slot",
                );
                i32_v
            } else if use_number_set {
                let v_box = lower_expr(ctx, value)?;
                guarded_set_number_has(ctx, &s_handle, &v_box)
            } else {
                if use_string_set {
                    let value_ref = lower_expr_native(
                        ctx,
                        value,
                        crate::native_value::ExpectedNativeRep::StringRef,
                    )?;
                    let i32_v = {
                        let blk = ctx.block();
                        let i32_v = blk.call(
                            I32,
                            "js_set_has_string",
                            &[(I64, &s_handle), (I64, &value_ref.value)],
                        );
                        i32_v
                    };
                    record_collection_string_key_selected(
                        ctx,
                        "SetHas",
                        "collection_string_key.set_has",
                        &value_ref.value,
                        "set",
                        "js_set_has_string",
                    );
                    record_collection_typed_value_selected(
                        ctx,
                        "SetHas",
                        "collection_typed_value.set_has_string",
                        &value_ref,
                        "set",
                        "string_value_helper",
                        "js_set_has_string",
                        "set_slot",
                    );
                    i32_v
                } else {
                    let v_box = lower_expr(ctx, value)?;
                    let i32_v = {
                        let blk = ctx.block();
                        blk.call(I32, "js_set_has", &[(I64, &s_handle), (DOUBLE, &v_box)])
                    };
                    if receiver_i32_set {
                        record_collection_typed_value_fallback(
                            ctx,
                            "SetHas",
                            "collection_typed_value.set_has_generic",
                            &v_box,
                            "set",
                            "int32_value_helper",
                            "js_set_has",
                            "value_expr_not_native_i32",
                        );
                    } else if receiver_u32_set {
                        record_collection_typed_value_fallback(
                            ctx,
                            "SetHas",
                            "collection_typed_value.set_has_generic",
                            &v_box,
                            "set",
                            "uint32_value_helper",
                            "js_set_has",
                            "value_expr_not_native_u32",
                        );
                    } else if receiver_f32_set {
                        record_collection_typed_value_fallback(
                            ctx,
                            "SetHas",
                            "collection_typed_value.set_has_generic",
                            &v_box,
                            "set",
                            "float32_value_helper",
                            "js_set_has",
                            "value_expr_not_native_f32",
                        );
                    } else if receiver_boolean_set {
                        record_collection_typed_value_fallback(
                            ctx,
                            "SetHas",
                            "collection_typed_value.set_has_generic",
                            &v_box,
                            "set",
                            "boolean_value_helper",
                            "js_set_has",
                            "value_expr_not_native_i1",
                        );
                    } else if receiver_number_set {
                        record_collection_number_key_fallback(
                            ctx,
                            "SetHas",
                            "collection_number_value.set_has_generic",
                            &v_box,
                            "set",
                            "number_value_helper",
                            "js_set_has",
                            "value_expr_not_numeric",
                            "value",
                        );
                    } else {
                        record_collection_string_key_fallback(
                            ctx,
                            "SetHas",
                            "collection_string_key.set_has_generic",
                            &v_box,
                            "set",
                            "js_set_has",
                            "receiver_or_value_not_static_string",
                        );
                    }
                    i32_v
                }
            };
            let blk = ctx.block();
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

        // -------- set.delete(value) -> boolean --------
        Expr::SetDelete { set, value } => {
            let receiver_i32_set = is_static_i32_set(ctx, set);
            let use_i32_set = receiver_i32_set && can_lower_i32_for_collection_value(ctx, value);
            let receiver_u32_set = is_static_u32_set(ctx, set);
            let use_u32_set = receiver_u32_set && can_lower_u32_for_collection_value(ctx, value);
            let receiver_f32_set = is_static_f32_set(ctx, set);
            let use_f32_set = receiver_f32_set && can_lower_f32_for_collection_value(value);
            let receiver_boolean_set = is_static_boolean_set(ctx, set);
            let use_boolean_set =
                receiver_boolean_set && can_lower_i1_for_collection_value(ctx, value);
            let receiver_number_set = is_static_number_set(ctx, set);
            let use_number_set = receiver_number_set && is_numeric_expr(ctx, value);
            let use_string_set =
                is_static_string_set(ctx, set) && is_definitely_string_expr(ctx, value);
            let s_box = lower_expr(ctx, set)?;
            let s_handle = {
                let blk = ctx.block();
                unbox_to_i64(blk, &s_box)
            };
            let i32_v = if use_i32_set {
                let value_i32 =
                    lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::I32)?;
                let i32_v = {
                    let blk = ctx.block();
                    blk.call(
                        I32,
                        "js_set_delete_i32",
                        &[(I64, &s_handle), (I32, &value_i32.value)],
                    )
                };
                record_collection_typed_value_selected(
                    ctx,
                    "SetDelete",
                    "collection_typed_value.set_delete_i32",
                    &value_i32,
                    "set",
                    "int32_value_helper",
                    "js_set_delete_i32",
                    "set_slot",
                );
                i32_v
            } else if use_u32_set {
                let value_u32 =
                    lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::U32)?;
                let i32_v = {
                    let blk = ctx.block();
                    blk.call(
                        I32,
                        "js_set_delete_u32",
                        &[(I64, &s_handle), (I32, &value_u32.value)],
                    )
                };
                record_collection_typed_value_selected(
                    ctx,
                    "SetDelete",
                    "collection_typed_value.set_delete_u32",
                    &value_u32,
                    "set",
                    "uint32_value_helper",
                    "js_set_delete_u32",
                    "set_slot",
                );
                i32_v
            } else if use_f32_set {
                let value_f32 =
                    lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::F32)?;
                let i32_v = {
                    let blk = ctx.block();
                    blk.call(
                        I32,
                        "js_set_delete_f32",
                        &[(I64, &s_handle), (F32, &value_f32.value)],
                    )
                };
                record_collection_typed_value_selected(
                    ctx,
                    "SetDelete",
                    "collection_typed_value.set_delete_f32",
                    &value_f32,
                    "set",
                    "float32_value_helper",
                    "js_set_delete_f32",
                    "set_slot",
                );
                i32_v
            } else if use_boolean_set {
                let value_i1 =
                    lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::I1)?;
                let i32_v = {
                    let blk = ctx.block();
                    let value_i32 = blk.zext(I1, &value_i1.value, I32);
                    blk.call(
                        I32,
                        "js_set_delete_bool",
                        &[(I64, &s_handle), (I32, &value_i32)],
                    )
                };
                record_collection_typed_value_selected(
                    ctx,
                    "SetDelete",
                    "collection_typed_value.set_delete_bool",
                    &value_i1,
                    "set",
                    "boolean_value_helper",
                    "js_set_delete_bool",
                    "set_slot",
                );
                i32_v
            } else if use_number_set {
                let v_box = lower_expr(ctx, value)?;
                guarded_set_number_delete(ctx, &s_handle, &v_box)
            } else {
                if use_string_set {
                    let value_ref = lower_expr_native(
                        ctx,
                        value,
                        crate::native_value::ExpectedNativeRep::StringRef,
                    )?;
                    let i32_v = {
                        let blk = ctx.block();
                        let i32_v = blk.call(
                            I32,
                            "js_set_delete_string",
                            &[(I64, &s_handle), (I64, &value_ref.value)],
                        );
                        i32_v
                    };
                    record_collection_string_key_selected(
                        ctx,
                        "SetDelete",
                        "collection_string_key.set_delete",
                        &value_ref.value,
                        "set",
                        "js_set_delete_string",
                    );
                    record_collection_typed_value_selected(
                        ctx,
                        "SetDelete",
                        "collection_typed_value.set_delete_string",
                        &value_ref,
                        "set",
                        "string_value_helper",
                        "js_set_delete_string",
                        "set_slot",
                    );
                    i32_v
                } else {
                    let v_box = lower_expr(ctx, value)?;
                    let i32_v = {
                        let blk = ctx.block();
                        blk.call(I32, "js_set_delete", &[(I64, &s_handle), (DOUBLE, &v_box)])
                    };
                    if receiver_i32_set {
                        record_collection_typed_value_fallback(
                            ctx,
                            "SetDelete",
                            "collection_typed_value.set_delete_generic",
                            &v_box,
                            "set",
                            "int32_value_helper",
                            "js_set_delete",
                            "value_expr_not_native_i32",
                        );
                    } else if receiver_u32_set {
                        record_collection_typed_value_fallback(
                            ctx,
                            "SetDelete",
                            "collection_typed_value.set_delete_generic",
                            &v_box,
                            "set",
                            "uint32_value_helper",
                            "js_set_delete",
                            "value_expr_not_native_u32",
                        );
                    } else if receiver_f32_set {
                        record_collection_typed_value_fallback(
                            ctx,
                            "SetDelete",
                            "collection_typed_value.set_delete_generic",
                            &v_box,
                            "set",
                            "float32_value_helper",
                            "js_set_delete",
                            "value_expr_not_native_f32",
                        );
                    } else if receiver_boolean_set {
                        record_collection_typed_value_fallback(
                            ctx,
                            "SetDelete",
                            "collection_typed_value.set_delete_generic",
                            &v_box,
                            "set",
                            "boolean_value_helper",
                            "js_set_delete",
                            "value_expr_not_native_i1",
                        );
                    } else if receiver_number_set {
                        record_collection_number_key_fallback(
                            ctx,
                            "SetDelete",
                            "collection_number_value.set_delete_generic",
                            &v_box,
                            "set",
                            "number_value_helper",
                            "js_set_delete",
                            "value_expr_not_numeric",
                            "value",
                        );
                    } else {
                        record_collection_string_key_fallback(
                            ctx,
                            "SetDelete",
                            "collection_string_key.set_delete_generic",
                            &v_box,
                            "set",
                            "js_set_delete",
                            "receiver_or_value_not_static_string",
                        );
                    }
                    i32_v
                }
            };
            let blk = ctx.block();
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

        // -------- set.size -> number --------
        Expr::SetSize(set) => {
            let s_box = lower_expr(ctx, set)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let i32_v = blk.call(I32, "js_set_size", &[(I64, &s_handle)]);
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }

        Expr::FsWriteFileSync(path, content) => {
            let p = lower_expr(ctx, path)?;
            let c = lower_expr(ctx, content)?;
            // js_fs_write_file_sync returns i32 (1=success). Discard the
            // result; fs.writeFileSync is void in JS.
            let _ = ctx
                .block()
                .call(I32, "js_fs_write_file_sync", &[(DOUBLE, &p), (DOUBLE, &c)]);
            Ok(double_literal(0.0))
        }

        Expr::FsAppendFileSync(path, content) => {
            // Issue #226. JS and WASM backends already had arms for this
            // HIR variant; the LLVM backend was missing the case here next
            // to `FsWriteFileSync`. The runtime fn
            // (`crates/perry-runtime/src/fs.rs::js_fs_append_file_sync`)
            // is correct; the codegen + lowerer plumbing was the gap.
            // The companion fix in `crates/perry-hir/src/lower/expr_call.rs`
            // extends the namespace-import path (`fs.appendFileSync` via
            // `import * as fs from "fs"`) — without that the variant
            // never gets emitted for the common usage shape. Returns i32
            // (1=success) which we discard; appendFileSync is void in JS.
            let p = lower_expr(ctx, path)?;
            let c = lower_expr(ctx, content)?;
            let _ = ctx
                .block()
                .call(I32, "js_fs_append_file_sync", &[(DOUBLE, &p), (DOUBLE, &c)]);
            Ok(double_literal(0.0))
        }

        // -------- NativeMethodCall (Phase H.1) --------
        // Perry's HIR uses NativeMethodCall { module, method, object, args }
        // for method calls on natively-typed receivers — specifically for
        // typed arrays (where `push`/`pop`/etc. on `T[]` get this shape
        // instead of the generic ArrayPush/Pop variants), and for
        // module-level calls (mysql.createConnection, redis.set, etc.).
        //
        // Phase H.1 handles the most common shape: `array.push_single`,
        // `array.push`, `array.pop_back` on typed arrays. The object is
        // a PropertyGet on a class instance (`this.items`) or a LocalGet.
        // We chain a get + push + set so reallocations are reflected
        // back in the source.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
