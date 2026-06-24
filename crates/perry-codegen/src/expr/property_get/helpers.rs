//! Free helper functions extracted from `property_get.rs`.
//!
//! Pure mechanical move — bodies are verbatim. Visibility widened to
//! `pub(crate)` so both the trunk's guarded arms and the sibling general
//! dispatch can reach them.

use super::*;

use anyhow::Result;
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
    is_numeric_expr, is_numeric_typed_array_class, is_set_expr, is_string_expr,
    is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

pub(crate) fn class_has_computed_runtime_members(ctx: &FnCtx<'_>, class_name: &str) -> bool {
    ctx.classes
        .get(class_name)
        .is_some_and(|class| !class.computed_members.is_empty())
}

pub(crate) fn lower_runtime_property_get_by_name(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
) -> Result<String> {
    let recv_box = lower_expr(ctx, object)?;
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let blk = ctx.block();
    let obj_bits = blk.bitcast_double_to_i64(&recv_box);
    let key_box = blk.load(DOUBLE, &key_handle_global);
    let key_bits = blk.bitcast_double_to_i64(&key_box);
    let key_handle = blk.and(I64, &key_bits, POINTER_MASK_I64);
    Ok(blk.call(
        DOUBLE,
        "js_object_get_field_by_name_f64",
        &[(I64, &obj_bits), (I64, &key_handle)],
    ))
}

pub(crate) fn lower_class_method_bind(
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

pub(crate) fn is_primitive_builtin_proto_method(builtin_name: &str, method_name: &str) -> bool {
    match builtin_name {
        "Number" => matches!(
            method_name,
            "toExponential" | "toFixed" | "toLocaleString" | "toPrecision" | "toString" | "valueOf"
        ),
        "Boolean" | "Symbol" => matches!(method_name, "toString" | "valueOf"),
        "BigInt" => matches!(method_name, "toString" | "valueOf"),
        _ => false,
    }
}

pub(crate) fn builtin_prototype_method_read<'a>(
    object: &'a Expr,
    property: &'a str,
) -> Option<(&'a str, &'a str)> {
    let Expr::PropertyGet {
        object: ctor_object,
        property: proto_property,
    } = object
    else {
        return None;
    };
    if proto_property != "prototype" {
        return None;
    }
    let Expr::PropertyGet {
        object: global_object,
        property: builtin_name,
    } = ctor_object.as_ref()
    else {
        return None;
    };
    if !matches!(global_object.as_ref(), Expr::GlobalGet(_)) {
        return None;
    }
    is_primitive_builtin_proto_method(builtin_name, property)
        .then_some((builtin_name.as_str(), property))
}

pub(crate) fn is_global_builtin_value_expr(expr: &Expr, name: &str) -> bool {
    matches!(
        expr,
        Expr::PropertyGet { object, property }
            if property == name && matches!(object.as_ref(), Expr::GlobalGet(_))
    )
}

pub(crate) fn promise_static_function_length_expr(expr: &Expr) -> Option<u32> {
    let Expr::PropertyGet { object, property } = expr else {
        return None;
    };
    let is_promise_receiver = matches!(object.as_ref(), Expr::GlobalGet(_))
        || is_global_builtin_value_expr(object, "Promise");
    if !is_promise_receiver {
        return None;
    }
    match property.as_str() {
        "withResolvers" => Some(0),
        "resolve" | "reject" | "all" | "race" | "allSettled" | "any" | "try" => Some(1),
        _ => None,
    }
}

pub(crate) fn lower_global_builtin_static_value(
    ctx: &mut FnCtx<'_>,
    builtin: &str,
    property: &str,
) -> String {
    if builtin == "Promise" {
        let key_idx = ctx.strings.intern(property);
        let key_bytes_global = format!("@{}", ctx.strings.entry(key_idx).bytes_global);
        let key_len = property.len().to_string();
        return ctx.block().call(
            DOUBLE,
            "js_promise_static_function_value",
            &[(PTR, &key_bytes_global), (I64, &key_len)],
        );
    }

    let builtin_idx = ctx.strings.intern(builtin);
    let builtin_bytes_global = format!("@{}", ctx.strings.entry(builtin_idx).bytes_global);
    let builtin_len = builtin.len().to_string();
    let builtin_value = ctx.block().call(
        DOUBLE,
        "js_get_global_this_builtin_value",
        &[(PTR, &builtin_bytes_global), (I64, &builtin_len)],
    );
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let blk = ctx.block();
    let builtin_handle = unbox_to_i64(blk, &builtin_value);
    let key_box = blk.load(DOUBLE, &key_handle_global);
    let key_bits = blk.bitcast_double_to_i64(&key_box);
    let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
    blk.call(
        DOUBLE,
        "js_object_get_field_by_name_f64",
        &[(I64, &builtin_handle), (I64, &key_raw)],
    )
}
