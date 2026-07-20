//! POD-record / scalar-replacement numeric-field analysis helpers.
//!
//! Split out of `type_analysis.rs` (file-size gate). Pure code move.

use super::*;

use perry_hir::{BinaryOp, Expr, UnaryOp};
use perry_types::Type as HirType;

use crate::expr::FnCtx;
use crate::type_analysis_class_fields::{
    class_field_declared_type, class_field_global_index, declared_field_type,
};
use crate::type_analysis_facts::{
    function_type_from_decl, hir_inferred_refinable_type, hir_inferred_static_type,
};
use crate::type_analysis_net::{net_result_class, net_result_type};

pub(crate) fn is_numeric_typed_array_class(name: &str) -> bool {
    matches!(
        name,
        "Int8Array"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Float16Array"
            | "Float32Array"
            | "Float64Array"
    )
}

pub(crate) fn expression_has_numeric_length(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    match static_type_of(ctx, object) {
        Some(HirType::Array(_)) | Some(HirType::Tuple(_)) | Some(HirType::String) => true,
        Some(HirType::Named(name)) => name == "Buffer" || is_numeric_typed_array_class(&name),
        _ => false,
    }
}

fn native_rep_materializes_to_js_number(rep: &crate::native_value::NativeRep) -> bool {
    matches!(
        rep,
        crate::native_value::NativeRep::I32
            | crate::native_value::NativeRep::I64
            | crate::native_value::NativeRep::U32
            | crate::native_value::NativeRep::U64
            | crate::native_value::NativeRep::USize
            | crate::native_value::NativeRep::F64
            | crate::native_value::NativeRep::F32
            | crate::native_value::NativeRep::U8
            | crate::native_value::NativeRep::BufferLen
            | crate::native_value::NativeRep::HandleId
    )
}

fn pod_record_local_has_materialized_object(ctx: &FnCtx<'_>, local_id: u32) -> bool {
    // Once a POD local has a materialized JS object path, later property
    // reads may observe mutable boxed object state instead of native bytes.
    ctx.native_rep_records.iter().any(|record| {
        record.local_id == Some(local_id) && record.consumer == "pod_record_materialize_object"
    })
}

pub(crate) fn pod_record_field_is_numeric(ctx: &FnCtx<'_>, object: &Expr, field: &str) -> bool {
    let Expr::LocalGet(id) = object else {
        return false;
    };
    if pod_record_local_has_materialized_object(ctx, *id) {
        return false;
    }
    ctx.pod_records
        .get(id)
        .and_then(|local| {
            local
                .layout
                .fields
                .iter()
                .find(|candidate| candidate.name == field)
        })
        .is_some_and(|field| native_rep_materializes_to_js_number(&field.native_rep))
}

fn collect_pod_numeric_field_read_locals(ctx: &FnCtx<'_>, expr: &Expr, out: &mut Vec<u32>) {
    match expr {
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::LocalGet(_))
            && pod_record_field_is_numeric(ctx, object, property) =>
        {
            if let Expr::LocalGet(id) = object.as_ref() {
                out.push(*id);
            }
        }
        Expr::PropertyGet { object, .. } => collect_pod_numeric_field_read_locals(ctx, object, out),
        Expr::PropertySet { object, value, .. } => {
            collect_pod_numeric_field_read_locals(ctx, object, out);
            collect_pod_numeric_field_read_locals(ctx, value, out);
        }
        Expr::IndexGet { object, index } => {
            collect_pod_numeric_field_read_locals(ctx, object, out);
            collect_pod_numeric_field_read_locals(ctx, index, out);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            collect_pod_numeric_field_read_locals(ctx, object, out);
            collect_pod_numeric_field_read_locals(ctx, index, out);
            collect_pod_numeric_field_read_locals(ctx, value, out);
        }
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
            collect_pod_numeric_field_read_locals(ctx, left, out);
            collect_pod_numeric_field_read_locals(ctx, right, out);
        }
        Expr::Unary { operand, .. } | Expr::TypeOf(operand) | Expr::Void(operand) => {
            collect_pod_numeric_field_read_locals(ctx, operand, out);
        }
        Expr::Logical { left, right, .. } => {
            collect_pod_numeric_field_read_locals(ctx, left, out);
            collect_pod_numeric_field_read_locals(ctx, right, out);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_pod_numeric_field_read_locals(ctx, condition, out);
            collect_pod_numeric_field_read_locals(ctx, then_expr, out);
            collect_pod_numeric_field_read_locals(ctx, else_expr, out);
        }
        Expr::Call { callee, args, .. } => {
            collect_pod_numeric_field_read_locals(ctx, callee, out);
            for arg in args {
                collect_pod_numeric_field_read_locals(ctx, arg, out);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(object) = object {
                collect_pod_numeric_field_read_locals(ctx, object, out);
            }
            for arg in args {
                collect_pod_numeric_field_read_locals(ctx, arg, out);
            }
        }
        Expr::New { args, .. } | Expr::NewDynamic { args, .. } => {
            for arg in args {
                collect_pod_numeric_field_read_locals(ctx, arg, out);
            }
        }
        Expr::Array(items) => {
            for item in items {
                collect_pod_numeric_field_read_locals(ctx, item, out);
            }
        }
        Expr::Object(items) => {
            for (_, item) in items {
                collect_pod_numeric_field_read_locals(ctx, item, out);
            }
        }
        _ => {}
    }
}

fn expr_may_materialize_pod_local(ctx: &FnCtx<'_>, expr: &Expr, target_id: u32) -> bool {
    match expr {
        Expr::LocalGet(id) => *id == target_id && ctx.pod_records.contains_key(id),
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::LocalGet(id) if *id == target_id)
            && ctx.pod_records.get(&target_id).is_some_and(|local| {
                local
                    .layout
                    .fields
                    .iter()
                    .any(|field| field.name == *property)
            }) =>
        {
            false
        }
        Expr::PropertyGet { object, .. } => expr_may_materialize_pod_local(ctx, object, target_id),
        Expr::PropertySet {
            object,
            property,
            value,
        } => {
            let pod_field_set = matches!(object.as_ref(), Expr::LocalGet(id) if *id == target_id)
                && ctx.pod_records.get(&target_id).is_some_and(|local| {
                    local
                        .layout
                        .fields
                        .iter()
                        .any(|field| field.name == *property)
                });
            pod_field_set
                || expr_may_materialize_pod_local(ctx, object, target_id)
                || expr_may_materialize_pod_local(ctx, value, target_id)
        }
        Expr::Call { callee, args, .. } => {
            expr_may_materialize_pod_local(ctx, callee, target_id)
                || args
                    .iter()
                    .any(|arg| expr_may_materialize_pod_local(ctx, arg, target_id))
        }
        Expr::NativeMethodCall { object, args, .. } => {
            object
                .as_ref()
                .is_some_and(|object| expr_may_materialize_pod_local(ctx, object, target_id))
                || args
                    .iter()
                    .any(|arg| expr_may_materialize_pod_local(ctx, arg, target_id))
        }
        Expr::IndexGet { object, index } => {
            expr_may_materialize_pod_local(ctx, object, target_id)
                || expr_may_materialize_pod_local(ctx, index, target_id)
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            expr_may_materialize_pod_local(ctx, object, target_id)
                || expr_may_materialize_pod_local(ctx, index, target_id)
                || expr_may_materialize_pod_local(ctx, value, target_id)
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            expr_may_materialize_pod_local(ctx, left, target_id)
                || expr_may_materialize_pod_local(ctx, right, target_id)
        }
        Expr::Unary { operand, .. } | Expr::TypeOf(operand) | Expr::Void(operand) => {
            expr_may_materialize_pod_local(ctx, operand, target_id)
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_may_materialize_pod_local(ctx, condition, target_id)
                || expr_may_materialize_pod_local(ctx, then_expr, target_id)
                || expr_may_materialize_pod_local(ctx, else_expr, target_id)
        }
        Expr::New { args, .. } | Expr::NewDynamic { args, .. } => args
            .iter()
            .any(|arg| expr_may_materialize_pod_local(ctx, arg, target_id)),
        Expr::Array(items) => items
            .iter()
            .any(|item| expr_may_materialize_pod_local(ctx, item, target_id)),
        Expr::Object(items) => items
            .iter()
            .any(|(_, item)| expr_may_materialize_pod_local(ctx, item, target_id)),
        _ => false,
    }
}

pub(crate) fn add_operands_have_pod_materialization_hazard(
    ctx: &FnCtx<'_>,
    left: &Expr,
    right: &Expr,
) -> bool {
    let mut right_pod_reads = Vec::new();
    collect_pod_numeric_field_read_locals(ctx, right, &mut right_pod_reads);
    right_pod_reads
        .into_iter()
        .any(|id| expr_may_materialize_pod_local(ctx, left, id))
}

fn static_object_property_type(ctx: &FnCtx<'_>, object: &Expr, field: &str) -> Option<HirType> {
    match static_type_of(ctx, object)? {
        HirType::Object(object_ty) => object_ty
            .properties
            .get(field)
            .map(|property| property.ty.clone()),
        _ => None,
    }
}

fn scalar_replaced_field_static_type(
    ctx: &FnCtx<'_>,
    object: &Expr,
    field: &str,
) -> Option<HirType> {
    match object {
        Expr::LocalGet(id)
            if ctx
                .scalar_replaced
                .get(id)
                .is_some_and(|fields| fields.contains_key(field)) =>
        {
            declared_field_type(ctx, object, field)
                .or_else(|| static_object_property_type(ctx, object, field))
        }
        Expr::This => {
            let target_id = ctx.scalar_ctor_target.last()?;
            if !ctx
                .scalar_replaced
                .get(target_id)
                .is_some_and(|fields| fields.contains_key(field))
            {
                return None;
            }
            ctx.class_stack
                .last()
                .and_then(|class_name| class_field_declared_type(ctx, class_name, field))
        }
        _ => None,
    }
}

pub(crate) fn scalar_replaced_field_is_raw_f64(
    ctx: &FnCtx<'_>,
    object: &Expr,
    field: &str,
) -> bool {
    scalar_replaced_field_static_type(ctx, object, field)
        .as_ref()
        .is_some_and(crate::typed_shape::type_is_raw_f64_candidate)
}

pub(crate) fn scalar_replaced_field_raw_f64_store_state(
    ctx: &FnCtx<'_>,
    local_id: Option<u32>,
    field: &str,
    declared_raw_f64: bool,
) -> bool {
    if !declared_raw_f64 {
        return false;
    }

    let field_note = format!("field={}", field);
    let mut proven_raw = false;
    for record in &ctx.native_rep_records {
        if record.local_id != local_id || !record.notes.iter().any(|note| note == &field_note) {
            continue;
        }
        match record.consumer.as_str() {
            "scalar_object_field_store.raw_f64" => {
                proven_raw = true;
            }
            "scalar_object_field_store"
                if record.notes.iter().any(|note| note == "raw_f64_field=1") =>
            {
                proven_raw = false;
            }
            _ => {}
        }
    }
    proven_raw
}

fn constant_array_index(index: &Expr) -> Option<usize> {
    match index {
        Expr::Integer(k) if *k >= 0 => Some(*k as usize),
        Expr::Number(f) if f.is_finite() && *f >= 0.0 && f.fract() == 0.0 => Some(*f as usize),
        _ => None,
    }
}

pub(crate) fn scalar_replaced_array_element_is_raw_f64(
    ctx: &FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> bool {
    let Expr::LocalGet(id) = object else {
        return false;
    };
    let Some(k) = constant_array_index(index) else {
        return false;
    };
    if ctx
        .scalar_replaced_arrays
        .get(id)
        .is_none_or(|slots| k >= slots.len())
    {
        return false;
    }
    match static_type_of(ctx, object) {
        Some(HirType::Array(elem)) => crate::typed_shape::type_is_raw_f64_candidate(elem.as_ref()),
        Some(HirType::Tuple(elems)) => elems
            .get(k)
            .is_some_and(crate::typed_shape::type_is_raw_f64_candidate),
        _ => false,
    }
}

fn type_has_numeric_pointer_free_array_layout_for_fallback(ty: &HirType) -> bool {
    match ty {
        HirType::Array(elem) => matches!(elem.as_ref(), HirType::Number | HirType::Int32),
        // #6011: `new Array<number>(n)` carries the generic spelling; its
        // element reads have the same boxed-fallback hazard as `Array(Number)`
        // (a hole surfaces `undefined` from the guarded read's boxed fallback,
        // which must be coerced before raw f64 arithmetic).
        HirType::Generic { base, type_args } if base == "Array" && type_args.len() == 1 => {
            matches!(type_args[0], HirType::Number | HirType::Int32)
        }
        HirType::Tuple(elems) => elems
            .iter()
            .all(|elem| matches!(elem, HirType::Number | HirType::Int32)),
        HirType::Union(variants) => variants.iter().all(|variant| {
            matches!(variant, HirType::Null | HirType::Void | HirType::Never)
                || type_has_numeric_pointer_free_array_layout_for_fallback(variant)
        }),
        _ => false,
    }
}

pub(crate) fn expr_may_return_boxed_value_from_raw_f64_fallback(
    ctx: &FnCtx<'_>,
    expr: &Expr,
) -> bool {
    match expr {
        Expr::PropertyGet {
            object, property, ..
        } => receiver_class_name(ctx, object)
            .and_then(|class_name| class_field_declared_type(ctx, &class_name, property))
            .as_ref()
            .is_some_and(crate::typed_shape::type_is_raw_f64_candidate),
        Expr::IndexGet { object, .. } => static_type_of(ctx, object)
            .as_ref()
            .is_some_and(type_has_numeric_pointer_free_array_layout_for_fallback),
        _ => false,
    }
}

pub(crate) fn is_fixed_width_buffer_numeric_read(method: &str) -> bool {
    matches!(
        method,
        "readUInt8"
            | "readUint8"
            | "readInt8"
            | "readUInt16BE"
            | "readUint16BE"
            | "readUInt16LE"
            | "readUint16LE"
            | "readInt16BE"
            | "readInt16LE"
            | "readUInt32BE"
            | "readUint32BE"
            | "readUInt32LE"
            | "readUint32LE"
            | "readInt32BE"
            | "readInt32LE"
            | "readFloatBE"
            | "readFloatLE"
            | "readDoubleBE"
            | "readDoubleLE"
    )
}
