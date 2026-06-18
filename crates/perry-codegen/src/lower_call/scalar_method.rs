//! Scalar-replaced receiver method summaries.

use anyhow::{bail, Result};
use perry_hir::Expr;
use perry_types::Type;

use crate::expr::{lower_expr, nanbox_pointer_inline, FnCtx};
use crate::native_value::{LoweredValue, NativeRep, SemanticKind};
use crate::types::{DOUBLE, I1, I32, I64, PTR};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScalarMethodArgKind {
    ProvenNumeric,
    GuardedF64Local,
    Generic,
}

fn scalar_method_arg_is_proven_numeric(arg: &Expr) -> bool {
    match arg {
        Expr::Integer(_) | Expr::Number(_) => true,
        Expr::Unary { op, operand } => {
            matches!(op, perry_hir::UnaryOp::Pos | perry_hir::UnaryOp::Neg)
                && scalar_method_arg_is_proven_numeric(operand)
        }
        Expr::Binary { op, left, right } => {
            matches!(
                op,
                perry_hir::BinaryOp::Add
                    | perry_hir::BinaryOp::Sub
                    | perry_hir::BinaryOp::Mul
                    | perry_hir::BinaryOp::Div
                    | perry_hir::BinaryOp::Mod
            ) && scalar_method_arg_is_proven_numeric(left)
                && scalar_method_arg_is_proven_numeric(right)
        }
        _ => false,
    }
}

fn scalar_method_arg_kind(ctx: &FnCtx<'_>, arg: &Expr) -> ScalarMethodArgKind {
    if scalar_method_arg_is_proven_numeric(arg) {
        return ScalarMethodArgKind::ProvenNumeric;
    }
    if let Expr::LocalGet(id) = arg {
        if ctx
            .local_types
            .get(id)
            .is_some_and(|ty| matches!(ty, Type::Number | Type::Int32))
        {
            return ScalarMethodArgKind::GuardedF64Local;
        }
    }
    ScalarMethodArgKind::Generic
}

fn lower_scalar_method_inline_body(
    ctx: &mut FnCtx<'_>,
    receiver_id: u32,
    class_name: &str,
    property: &str,
    method: &perry_hir::Function,
    arg_values: &[String],
) -> Result<String> {
    let saved_locals = ctx.locals.clone();
    let saved_local_types = ctx.local_types.clone();
    let saved_this_len = ctx.this_stack.len();
    let saved_class_len = ctx.class_stack.len();
    let saved_scalar_ctor_len = ctx.scalar_ctor_target.len();

    for (param, value) in method.params.iter().zip(arg_values.iter()) {
        let slot = ctx.func.alloca_entry(DOUBLE);
        ctx.block().store(DOUBLE, value, &slot);
        ctx.locals.insert(param.id, slot);
        ctx.local_types.insert(param.id, param.ty.clone());
    }

    ctx.scalar_ctor_target.push(receiver_id);
    ctx.class_stack.push(class_name.to_string());
    let dummy_this = ctx.func.alloca_entry(DOUBLE);
    ctx.this_stack.push(dummy_this);

    let result = match method.body.as_slice() {
        [perry_hir::Stmt::Return(Some(expr))] => lower_expr(ctx, expr)?,
        _ => unreachable!("simple scalar method summary only accepts one return"),
    };

    ctx.this_stack.truncate(saved_this_len);
    ctx.class_stack.truncate(saved_class_len);
    ctx.scalar_ctor_target.truncate(saved_scalar_ctor_len);
    ctx.locals = saved_locals;
    ctx.local_types = saved_local_types;

    let lowered = LoweredValue {
        semantic: SemanticKind::JsValue,
        rep: NativeRep::JsValue,
        llvm_ty: DOUBLE,
        value: result.clone(),
    };
    ctx.record_lowered_value(
        "ScalarMethodCall",
        Some(receiver_id),
        "scalar_method_summary_inline",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        vec![
            format!("class={class_name}"),
            format!("method={property}"),
            "receiver=scalar_replaced".to_string(),
        ],
    );

    Ok(result)
}

fn materialize_scalar_receiver(
    ctx: &mut FnCtx<'_>,
    receiver_id: u32,
    class_name: &str,
) -> Result<String> {
    let Some(class_id) = ctx.class_ids.get(class_name).copied() else {
        bail!("cannot materialize scalar receiver for class without class id: {class_name}");
    };
    let mut field_slots: Vec<(String, String)> = ctx
        .scalar_replaced
        .get(&receiver_id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "cannot materialize missing scalar receiver local {} for class {}",
                receiver_id,
                class_name
            )
        })?
        .iter()
        .map(|(field, slot)| (field.clone(), slot.clone()))
        .collect();
    field_slots.sort_by(|left, right| left.0.cmp(&right.0));
    let field_count = ctx
        .class_field_counts
        .get(class_name)
        .copied()
        .unwrap_or(field_slots.len() as u32)
        .max(field_slots.len() as u32);
    let class_id_str = class_id.to_string();
    let field_count_str = field_count.to_string();
    let obj_handle = ctx.block().call(
        I64,
        "js_object_alloc",
        &[(I32, &class_id_str), (I32, &field_count_str)],
    );

    for (field, slot) in field_slots {
        let value = ctx.block().load(DOUBLE, &slot);
        let key_idx = ctx.strings.intern(&field);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let key_box = ctx.block().load(DOUBLE, &key_handle_global);
        let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
        let key_raw = ctx
            .block()
            .and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
        ctx.block().call_void(
            "js_object_set_field_by_name",
            &[(I64, &obj_handle), (I64, &key_raw), (DOUBLE, &value)],
        );
    }

    Ok(nanbox_pointer_inline(ctx.block(), &obj_handle))
}

fn lower_materialized_receiver_dispatch(
    ctx: &mut FnCtx<'_>,
    receiver_id: u32,
    class_name: &str,
    property: &str,
    lowered_args: &[String],
) -> Result<String> {
    let recv_box = materialize_scalar_receiver(ctx, receiver_id, class_name)?;
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let key_box = ctx.block().load(DOUBLE, &key_handle_global);
    let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
    let method_id = ctx
        .block()
        .and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
    let (args_ptr, args_len) = if lowered_args.is_empty() {
        ("null".to_string(), "0".to_string())
    } else {
        let n = lowered_args.len();
        let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
        for (i, value) in lowered_args.iter().enumerate() {
            let slot = ctx.block().gep(DOUBLE, &buf_reg, &[(I64, &i.to_string())]);
            ctx.block().store(DOUBLE, value, &slot);
        }
        let ptr_reg = ctx.block().next_reg();
        ctx.block().emit_raw(format!(
            "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
            ptr_reg, n, buf_reg
        ));
        (ptr_reg, n.to_string())
    };
    Ok(ctx.block().call(
        DOUBLE,
        "js_native_call_method_by_id",
        &[
            (DOUBLE, &recv_box),
            (I64, &method_id),
            (PTR, &args_ptr),
            (I64, &args_len),
        ],
    ))
}

pub(super) fn try_lower_scalar_replaced_method_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    let Expr::PropertyGet { object, property } = callee else {
        return Ok(None);
    };
    let Expr::LocalGet(receiver_id) = object.as_ref() else {
        return Ok(None);
    };
    if !ctx.scalar_replaced.contains_key(receiver_id) {
        return Ok(None);
    }
    let Some(class_name) = crate::type_analysis::receiver_class_name(ctx, object.as_ref()) else {
        return Ok(None);
    };
    let Some(method) = crate::collectors::simple_scalar_method_summary(
        ctx.classes,
        &class_name,
        property,
        args.len(),
    )
    .cloned() else {
        return Ok(None);
    };
    let arg_kinds: Vec<_> = args
        .iter()
        .map(|arg| scalar_method_arg_kind(ctx, arg))
        .collect();

    let mut lowered_args = Vec::with_capacity(args.len());
    for arg in args {
        lowered_args.push(lower_expr(ctx, arg)?);
    }

    if arg_kinds
        .iter()
        .any(|kind| matches!(kind, ScalarMethodArgKind::Generic))
    {
        return Ok(Some(lower_materialized_receiver_dispatch(
            ctx,
            *receiver_id,
            &class_name,
            property,
            &lowered_args,
        )?));
    }

    if !arg_kinds
        .iter()
        .any(|kind| matches!(kind, ScalarMethodArgKind::GuardedF64Local))
    {
        return Ok(Some(lower_scalar_method_inline_body(
            ctx,
            *receiver_id,
            &class_name,
            property,
            &method,
            &lowered_args,
        )?));
    }

    let mut guard: Option<String> = None;
    for (kind, value) in arg_kinds.iter().zip(lowered_args.iter()) {
        if !matches!(kind, ScalarMethodArgKind::GuardedF64Local) {
            continue;
        }
        let raw = ctx
            .block()
            .call(I32, "js_typed_f64_arg_guard", &[(DOUBLE, value.as_str())]);
        let ok = ctx.block().icmp_ne(I32, &raw, "0");
        guard = Some(match guard {
            Some(prev) => ctx.block().and(I1, &prev, &ok),
            None => ok,
        });
    }

    let fast_idx = ctx.new_block("scalar_method_arg_guard.fast");
    let fallback_idx = ctx.new_block("scalar_method_arg_guard.fallback");
    let merge_idx = ctx.new_block("scalar_method_arg_guard.merge");
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    if let Some(guard) = guard {
        ctx.block().cond_br(&guard, &fast_label, &fallback_label);
    } else {
        ctx.block().br(&fast_label);
    }

    ctx.current_block = fast_idx;
    let mut fast_args = Vec::with_capacity(lowered_args.len());
    for (kind, value) in arg_kinds.iter().zip(lowered_args.iter()) {
        if matches!(kind, ScalarMethodArgKind::GuardedF64Local) {
            fast_args.push(ctx.block().call(
                DOUBLE,
                "js_typed_f64_arg_to_raw",
                &[(DOUBLE, value.as_str())],
            ));
        } else {
            fast_args.push(value.clone());
        }
    }
    let fast_value = lower_scalar_method_inline_body(
        ctx,
        *receiver_id,
        &class_name,
        property,
        &method,
        &fast_args,
    )?;
    let after_fast = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = fallback_idx;
    let fallback_value = lower_materialized_receiver_dispatch(
        ctx,
        *receiver_id,
        &class_name,
        property,
        &lowered_args,
    )?;
    let after_fallback = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    Ok(Some(ctx.block().phi(
        DOUBLE,
        &[
            (fast_value.as_str(), after_fast.as_str()),
            (fallback_value.as_str(), after_fallback.as_str()),
        ],
    )))
}
