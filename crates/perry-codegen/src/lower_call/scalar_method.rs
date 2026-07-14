//! Scalar-replaced receiver method summaries.

use anyhow::{bail, Result};
use std::collections::HashMap;

use perry_hir::{BinaryOp, Expr, UnaryOp};
use perry_types::Type;

use crate::expr::{
    emit_jsvalue_slot_store_on_block, i32_to_nanbox, lower_expr, lower_expr_as_i32,
    nanbox_pointer_inline, FnCtx,
};
use crate::native_value::{
    BufferAccessMode, LoweredValue, MaterializationReason, NativeFactUse, NativeRep, SemanticKind,
};
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScalarMethodArgKind {
    ProvenNumeric,
    GuardedF64Expr,
    GuardedI32Local,
    Generic,
}

#[derive(Clone, Debug)]
struct ScalarMethodArgPlan {
    kind: ScalarMethodArgKind,
    guard_locals: Vec<u32>,
    expression_guard: bool,
}

fn push_guard_local_once(locals: &mut Vec<u32>, id: u32) {
    if !locals.contains(&id) {
        locals.push(id);
    }
}

fn collect_guarded_numeric_arg_locals(ctx: &FnCtx<'_>, arg: &Expr) -> Option<Vec<u32>> {
    fn walk(ctx: &FnCtx<'_>, arg: &Expr, locals: &mut Vec<u32>) -> bool {
        match arg {
            Expr::Integer(_) | Expr::Number(_) => true,
            Expr::LocalGet(id) => {
                if ctx.closure_captures.contains_key(id)
                    || ctx.boxed_vars.contains(id)
                    || ctx.module_globals.contains_key(id)
                    || !ctx.locals.contains_key(id)
                    || !ctx
                        .local_types
                        .get(id)
                        .is_some_and(|ty| matches!(ty, Type::Number | Type::Int32))
                {
                    return false;
                }
                push_guard_local_once(locals, *id);
                true
            }
            Expr::Unary { op, operand } => {
                matches!(op, UnaryOp::Pos | UnaryOp::Neg) && walk(ctx, operand, locals)
            }
            Expr::Binary { op, left, right } => {
                matches!(
                    op,
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod
                ) && walk(ctx, left, locals)
                    && walk(ctx, right, locals)
            }
            _ => false,
        }
    }

    let mut locals = Vec::new();
    if walk(ctx, arg, &mut locals) {
        Some(locals)
    } else {
        None
    }
}

fn local_can_use_public_arg_guard(ctx: &FnCtx<'_>, id: u32, expected: Type) -> bool {
    !ctx.closure_captures.contains_key(&id)
        && !ctx.boxed_vars.contains(&id)
        && !ctx.module_globals.contains_key(&id)
        && ctx.locals.contains_key(&id)
        && ctx.local_types.get(&id).is_some_and(|ty| *ty == expected)
}

fn scalar_method_arg_plan(ctx: &FnCtx<'_>, arg: &Expr, param_ty: &Type) -> ScalarMethodArgPlan {
    if matches!(param_ty, Type::Int32) {
        return match arg {
            Expr::Integer(value) if i32::try_from(*value).is_ok() => ScalarMethodArgPlan {
                kind: ScalarMethodArgKind::ProvenNumeric,
                guard_locals: Vec::new(),
                expression_guard: false,
            },
            Expr::LocalGet(id) if local_can_use_public_arg_guard(ctx, *id, Type::Int32) => {
                ScalarMethodArgPlan {
                    kind: ScalarMethodArgKind::GuardedI32Local,
                    guard_locals: vec![*id],
                    expression_guard: false,
                }
            }
            _ => ScalarMethodArgPlan {
                kind: ScalarMethodArgKind::Generic,
                guard_locals: Vec::new(),
                expression_guard: false,
            },
        };
    }

    match collect_guarded_numeric_arg_locals(ctx, arg) {
        Some(guard_locals) if guard_locals.is_empty() => ScalarMethodArgPlan {
            kind: ScalarMethodArgKind::ProvenNumeric,
            guard_locals,
            expression_guard: false,
        },
        Some(guard_locals) => ScalarMethodArgPlan {
            kind: ScalarMethodArgKind::GuardedF64Expr,
            guard_locals,
            expression_guard: !matches!(arg, Expr::LocalGet(_)),
        },
        None => ScalarMethodArgPlan {
            kind: ScalarMethodArgKind::Generic,
            guard_locals: Vec::new(),
            expression_guard: false,
        },
    }
}

fn lower_int32_scalar_arg_fast(
    ctx: &mut FnCtx<'_>,
    arg: &Expr,
    raw_i32_locals: &HashMap<u32, String>,
) -> Result<String> {
    match arg {
        Expr::Integer(value) => i32::try_from(*value)
            .map(|value| value.to_string())
            .map_err(|_| anyhow::anyhow!("scalar Int32 method literal out of range: {value}")),
        Expr::LocalGet(id) => raw_i32_locals
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing guarded scalar Int32 method arg local {id}")),
        _ => lower_expr_as_i32(ctx, arg),
    }
}

fn lower_guarded_numeric_arg_fast(
    ctx: &mut FnCtx<'_>,
    arg: &Expr,
    raw_locals: &HashMap<u32, String>,
) -> Result<String> {
    match arg {
        Expr::Integer(_) | Expr::Number(_) => lower_expr(ctx, arg),
        Expr::LocalGet(id) => raw_locals
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing guarded scalar method arg local {id}")),
        Expr::Unary { op, operand } => {
            let value = lower_guarded_numeric_arg_fast(ctx, operand, raw_locals)?;
            Ok(match op {
                UnaryOp::Pos => value,
                UnaryOp::Neg => ctx.block().fneg(&value),
                _ => bail!("unsupported guarded scalar method unary arg"),
            })
        }
        Expr::Binary { op, left, right } => {
            let left = lower_guarded_numeric_arg_fast(ctx, left, raw_locals)?;
            let right = lower_guarded_numeric_arg_fast(ctx, right, raw_locals)?;
            Ok(match op {
                BinaryOp::Add => ctx.block().fadd(&left, &right),
                BinaryOp::Sub => ctx.block().fsub(&left, &right),
                BinaryOp::Mul => ctx.block().fmul(&left, &right),
                BinaryOp::Div => ctx.block().fdiv(&left, &right),
                BinaryOp::Mod => ctx.block().frem(&left, &right),
                _ => bail!("unsupported guarded scalar method binary arg"),
            })
        }
        _ => bail!(
            "unsupported guarded scalar method arg expression kind {}",
            crate::expr::variant_name(arg)
        ),
    }
}

fn load_scalar_method_arg_guard_value(ctx: &mut FnCtx<'_>, id: u32) -> Result<String> {
    let slot = ctx
        .locals
        .get(&id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("missing scalar method arg guard local {id}"))?;
    Ok(ctx.block().load(DOUBLE, &slot))
}

fn collect_guard_local_values(
    ctx: &mut FnCtx<'_>,
    arg_plans: &[ScalarMethodArgPlan],
) -> Result<Vec<(u32, String)>> {
    let mut values = Vec::new();
    let mut seen = Vec::new();
    for plan in arg_plans {
        if !matches!(plan.kind, ScalarMethodArgKind::GuardedF64Expr) {
            continue;
        }
        for id in &plan.guard_locals {
            if seen.contains(id) {
                continue;
            }
            seen.push(*id);
            values.push((*id, load_scalar_method_arg_guard_value(ctx, *id)?));
        }
    }
    Ok(values)
}

fn collect_i32_guard_local_values(
    ctx: &mut FnCtx<'_>,
    arg_plans: &[ScalarMethodArgPlan],
) -> Result<Vec<(u32, String)>> {
    let mut values = Vec::new();
    let mut seen = Vec::new();
    for plan in arg_plans {
        if !matches!(plan.kind, ScalarMethodArgKind::GuardedI32Local) {
            continue;
        }
        for id in &plan.guard_locals {
            if seen.contains(id) {
                continue;
            }
            seen.push(*id);
            values.push((*id, load_scalar_method_arg_guard_value(ctx, *id)?));
        }
    }
    Ok(values)
}

fn scalar_method_summary_fact(
    receiver_id: u32,
    class_name: &str,
    property: &str,
    state: &'static str,
    detail: &'static str,
) -> NativeFactUse {
    NativeFactUse {
        fact_id: format!(
            "native_region.scalar_method_summary.{receiver_id}.{class_name}.{property}"
        ),
        kind: "scalar_method_summary".to_string(),
        local_id: Some(receiver_id),
        state: state.to_string(),
        detail: detail.to_string(),
        reason: None,
    }
}

fn scalar_method_notes(class_name: &str, property: &str) -> Vec<String> {
    vec![
        format!("class={class_name}"),
        format!("method={property}"),
        "receiver=scalar_replaced".to_string(),
    ]
}

fn scalar_method_return_note(method: &perry_hir::Function) -> &'static str {
    match method.return_type {
        Type::Int32 => "summary_return=int32",
        Type::Boolean => "summary_return=boolean",
        _ => "summary_return=number",
    }
}

fn lower_scalar_method_inline_body(
    ctx: &mut FnCtx<'_>,
    receiver_id: u32,
    class_name: &str,
    property: &str,
    method: &perry_hir::Function,
    arg_values: &[String],
    fact_detail: &'static str,
    extra_notes: Vec<String>,
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

    let mut result = None;
    for stmt in &method.body {
        match stmt {
            perry_hir::Stmt::Let {
                id,
                ty,
                init: Some(init),
                ..
            } => {
                let value = lower_expr(ctx, init)?;
                let slot = ctx.func.alloca_entry(DOUBLE);
                ctx.block().store(DOUBLE, &value, &slot);
                ctx.locals.insert(*id, slot);
                ctx.local_types.insert(*id, ty.clone());
            }
            perry_hir::Stmt::Return(Some(expr)) => {
                result = Some(lower_expr(ctx, expr)?);
                break;
            }
            _ => unreachable!("simple scalar method summary only accepts lets and one return"),
        }
    }
    let result = result.expect("simple scalar method summary must return a value");

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
    let mut notes = scalar_method_notes(class_name, property);
    notes.push(scalar_method_return_note(method).to_string());
    notes.extend(extra_notes);
    ctx.record_lowered_value_with_access_mode_and_facts(
        "ScalarMethodCall",
        Some(receiver_id),
        "scalar_method_summary_inline",
        &lowered,
        None,
        None,
        None,
        None,
        None,
        None,
        vec![scalar_method_summary_fact(
            receiver_id,
            class_name,
            property,
            "consumed",
            fact_detail,
        )],
        Vec::new(),
        false,
        false,
        notes,
    );

    Ok(result)
}

fn lower_scalar_method_int32_inline_body(
    ctx: &mut FnCtx<'_>,
    receiver_id: u32,
    class_name: &str,
    property: &str,
    method: &perry_hir::Function,
    arg_values: &[String],
    fact_detail: &'static str,
    extra_notes: Vec<String>,
) -> Result<String> {
    let saved_locals = ctx.locals.clone();
    let saved_local_types = ctx.local_types.clone();
    let saved_i32_slots = ctx.i32_counter_slots.clone();
    let saved_this_len = ctx.this_stack.len();
    let saved_class_len = ctx.class_stack.len();
    let saved_scalar_ctor_len = ctx.scalar_ctor_target.len();

    for (param, value) in method.params.iter().zip(arg_values.iter()) {
        let slot = ctx.func.alloca_entry(I32);
        ctx.block().store(I32, value, &slot);
        ctx.i32_counter_slots.insert(param.id, slot);
        ctx.local_types.insert(param.id, param.ty.clone());
    }

    ctx.scalar_ctor_target.push(receiver_id);
    ctx.class_stack.push(class_name.to_string());
    let dummy_this = ctx.func.alloca_entry(DOUBLE);
    ctx.this_stack.push(dummy_this);

    let mut raw_i32 = None;
    for stmt in &method.body {
        match stmt {
            perry_hir::Stmt::Let {
                id,
                ty,
                init: Some(init),
                ..
            } => {
                let value = lower_expr_as_i32(ctx, init)?;
                let slot = ctx.func.alloca_entry(I32);
                ctx.block().store(I32, &value, &slot);
                ctx.i32_counter_slots.insert(*id, slot);
                ctx.local_types.insert(*id, ty.clone());
            }
            perry_hir::Stmt::Return(Some(expr)) => {
                raw_i32 = Some(lower_expr_as_i32(ctx, expr)?);
                break;
            }
            _ => unreachable!("simple scalar method summary only accepts lets and one return"),
        }
    }
    let raw_i32 = raw_i32.expect("simple scalar method summary must return a value");
    let result = i32_to_nanbox(ctx.block(), &raw_i32);

    ctx.this_stack.truncate(saved_this_len);
    ctx.class_stack.truncate(saved_class_len);
    ctx.scalar_ctor_target.truncate(saved_scalar_ctor_len);
    ctx.locals = saved_locals;
    ctx.local_types = saved_local_types;
    ctx.i32_counter_slots = saved_i32_slots;

    let lowered = LoweredValue {
        semantic: SemanticKind::JsValue,
        rep: NativeRep::JsValue,
        llvm_ty: DOUBLE,
        value: result.clone(),
    };
    let mut notes = scalar_method_notes(class_name, property);
    notes.push(scalar_method_return_note(method).to_string());
    notes.extend(extra_notes);
    ctx.record_lowered_value_with_access_mode_and_facts(
        "ScalarMethodCall",
        Some(receiver_id),
        "scalar_method_summary_inline",
        &lowered,
        None,
        None,
        None,
        None,
        None,
        None,
        vec![scalar_method_summary_fact(
            receiver_id,
            class_name,
            property,
            "consumed",
            fact_detail,
        )],
        Vec::new(),
        false,
        false,
        notes,
    );

    Ok(result)
}

fn record_scalar_method_materialized_fallback(
    ctx: &mut FnCtx<'_>,
    receiver_id: u32,
    class_name: &str,
    property: &str,
    value: &str,
    fallback_state: &'static str,
    guard_note: Option<&'static str>,
) {
    let lowered = LoweredValue {
        semantic: SemanticKind::JsValue,
        rep: NativeRep::JsValue,
        llvm_ty: DOUBLE,
        value: value.to_string(),
    };
    let mut notes = scalar_method_notes(class_name, property);
    notes.push(format!("scalar_method_fallback={fallback_state}"));
    if let Some(guard) = guard_note {
        notes.push(format!("arg_guard={guard}"));
    }
    ctx.record_lowered_value_with_access_mode_and_facts(
        "ScalarMethodCall",
        Some(receiver_id),
        "scalar_method_summary_materialized_fallback",
        &lowered,
        None,
        None,
        Some(BufferAccessMode::DynamicFallback),
        Some(MaterializationReason::RuntimeApi),
        None,
        None,
        Vec::new(),
        vec![scalar_method_summary_fact(
            receiver_id,
            class_name,
            property,
            fallback_state,
            match fallback_state {
                "generic_arg" => "generic_argument",
                "arg_guard_failed" => "guarded_numeric_args_fallback",
                _ => fallback_state,
            },
        )],
        false,
        false,
        notes,
    );
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
    let parent_class_id = ctx
        .classes
        .get(class_name)
        .and_then(|class| class.extends_name.as_deref())
        .and_then(|parent| ctx.class_ids.get(parent).copied())
        .unwrap_or(0);
    let parent_class_id_str = parent_class_id.to_string();
    let (obj_handle, has_stable_keys) =
        if let Some(keys_global_name) = ctx.class_keys_globals.get(class_name).cloned() {
            let keys_slot = if let Some(slot) = ctx.class_keys_slots.get(class_name).cloned() {
                slot
            } else {
                let slot = ctx.func.entry_init_load_global(&keys_global_name, I64);
                ctx.class_keys_slots
                    .insert(class_name.to_string(), slot.clone());
                slot
            };
            let keys_ptr = ctx.block().load(I64, &keys_slot);
            ctx.pending_declares.push((
                "js_object_alloc_class_inline_keys".to_string(),
                I64,
                vec![I32, I32, I32, I64],
            ));
            let obj_handle = ctx.block().call(
                I64,
                "js_object_alloc_class_inline_keys",
                &[
                    (I32, &class_id_str),
                    (I32, &parent_class_id_str),
                    (I32, &field_count_str),
                    (I64, &keys_ptr),
                ],
            );
            emit_materialized_scalar_receiver_typed_shape_init(ctx, class_name, &obj_handle);
            (obj_handle, true)
        } else {
            (
                ctx.block().call(
                    I64,
                    "js_object_alloc",
                    &[(I32, &class_id_str), (I32, &field_count_str)],
                ),
                false,
            )
        };

    for (field, slot) in field_slots {
        let value = ctx.block().load(DOUBLE, &slot);
        if let (true, Some(field_index)) = (
            has_stable_keys,
            crate::type_analysis::class_field_global_index(ctx, class_name, &field),
        ) {
            emit_materialized_scalar_receiver_direct_field_store(
                ctx,
                receiver_id,
                class_name,
                &field,
                field_index,
                &obj_handle,
                &value,
            );
        } else {
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
    }

    Ok(nanbox_pointer_inline(ctx.block(), &obj_handle))
}

fn emit_materialized_scalar_receiver_typed_shape_init(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    obj_handle: &str,
) {
    let Some(keys_global_name) = ctx.class_keys_globals.get(class_name).cloned() else {
        return;
    };
    // Refs #5094: prefer the prefix-disambiguated chain so slot/word counts
    // agree with the mask globals emitted in compile_module (same-named
    // cross-module parents mis-resolve in the name-keyed walk).
    let typed_layout = ctx
        .class_init_chains
        .get(class_name)
        .map(|chain| crate::typed_shape::class_typed_layout_from_chain(chain))
        .unwrap_or_else(|| crate::typed_shape::class_typed_layout(ctx.classes, class_name));
    let slot_count_str = typed_layout.slot_count.to_string();
    let raw_mask_word_count_str = typed_layout.raw_f64_mask_words.len().to_string();
    let pointer_mask_word_count_str = typed_layout.pointer_mask_words.len().to_string();
    let raw_mask_ref = if typed_layout.raw_f64_mask_words.is_empty() {
        "null".to_string()
    } else {
        format!(
            "@{}",
            crate::typed_shape::raw_f64_mask_global_name_from_keys_global(&keys_global_name)
        )
    };
    let pointer_mask_ref = if typed_layout.pointer_mask_words.is_empty() {
        "null".to_string()
    } else {
        format!(
            "@{}",
            crate::typed_shape::mask_global_name_from_keys_global(&keys_global_name)
        )
    };
    ctx.block().call_void(
        "js_gc_init_typed_shape_layout",
        &[
            (I64, obj_handle),
            (I32, &slot_count_str),
            (PTR, &raw_mask_ref),
            (I32, &raw_mask_word_count_str),
            (PTR, &pointer_mask_ref),
            (I32, &pointer_mask_word_count_str),
        ],
    );
}

fn emit_materialized_scalar_receiver_direct_field_store(
    ctx: &mut FnCtx<'_>,
    receiver_id: u32,
    class_name: &str,
    field: &str,
    field_index: u32,
    obj_handle: &str,
    value: &str,
) {
    let field_idx_str = field_index.to_string();
    let field_ptr = {
        let blk = ctx.block();
        let obj_ptr = blk.inttoptr(I64, obj_handle);
        let fields_base = blk.gep(I8, &obj_ptr, &[(I64, "24")]);
        blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)])
    };
    let is_raw_f64 = crate::type_analysis::class_field_declared_type(ctx, class_name, field)
        .as_ref()
        .is_some_and(crate::typed_shape::type_is_raw_f64_candidate);
    let stored = if is_raw_f64 {
        let raw = ctx.block().call(
            DOUBLE,
            "js_array_numeric_value_to_raw_f64",
            &[(DOUBLE, value)],
        );
        // GC_STORE_AUDIT(POINTER_FREE): raw-f64 class-field store — the field's
        // declared type is a raw-f64 candidate and `raw` is a canonicalized
        // numeric f64 (`js_array_numeric_value_to_raw_f64`). A number is never a
        // GC pointer, so the field slot carries no heap edge and needs no barrier.
        ctx.block().store(DOUBLE, &raw, &field_ptr);
        LoweredValue::f64(raw)
    } else {
        let field_addr = ctx.block().ptrtoint(&field_ptr, I64);
        emit_jsvalue_slot_store_on_block(
            ctx.block(),
            &field_ptr,
            value,
            obj_handle,
            &field_idx_str,
            true,
            obj_handle,
            &field_addr,
            true,
        );
        LoweredValue {
            semantic: SemanticKind::JsValue,
            rep: NativeRep::JsValue,
            llvm_ty: DOUBLE,
            value: value.to_string(),
        }
    };
    let mut notes = scalar_method_notes(class_name, "<materialize>");
    notes.push(format!("field={field}"));
    notes.push(format!("field_index={field_idx_str}"));
    notes.push("receiver_materialization=direct_slot".to_string());
    notes.push("field_layout=fixed_slot_array".to_string());
    notes.push(format!("raw_f64_field={}", is_raw_f64 as u8));
    if is_raw_f64 {
        notes.push("pointer_bitmap=non_pointer".to_string());
        notes.push("write_barrier=elided_raw_f64".to_string());
    } else {
        notes.push("write_barrier=emitted_conservative".to_string());
    }
    ctx.record_lowered_value_with_access_mode(
        "ScalarReceiverMaterializeField",
        Some(receiver_id),
        "scalar_receiver_materialize.direct_field_store",
        &stored,
        None,
        None,
        Some(BufferAccessMode::CheckedNative),
        Some(MaterializationReason::RuntimeApi),
        false,
        false,
        notes,
    );
    if is_raw_f64 {
        let mut barrier_notes = scalar_method_notes(class_name, "<materialize>");
        barrier_notes.push("reason=scalar_receiver_raw_f64_field_pointer_free".to_string());
        barrier_notes.push(format!("field={field}"));
        barrier_notes.push(format!("field_index={field_idx_str}"));
        barrier_notes.push("receiver_materialization=direct_slot".to_string());
        barrier_notes.push("field_layout=raw_f64_slot_array".to_string());
        barrier_notes.push("pointer_bitmap=non_pointer".to_string());
        ctx.record_lowered_value_with_access_mode(
            "WriteBarrierElided",
            Some(receiver_id),
            "write_barrier.elided_scalar_receiver_materialize_raw_f64",
            &stored,
            None,
            None,
            None,
            Some(MaterializationReason::RuntimeApi),
            false,
            false,
            barrier_notes,
        );
    }
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

fn lower_scalar_replaced_int32_method_call(
    ctx: &mut FnCtx<'_>,
    receiver_id: u32,
    class_name: &str,
    property: &str,
    method: &perry_hir::Function,
    args: &[Expr],
) -> Result<String> {
    let arg_plans: Vec<_> = args
        .iter()
        .zip(method.params.iter())
        .map(|(arg, param)| scalar_method_arg_plan(ctx, arg, &param.ty))
        .collect();

    if arg_plans
        .iter()
        .any(|plan| matches!(plan.kind, ScalarMethodArgKind::Generic))
    {
        let mut lowered_args = Vec::with_capacity(args.len());
        for arg in args {
            lowered_args.push(lower_expr(ctx, arg)?);
        }
        let fallback = lower_materialized_receiver_dispatch(
            ctx,
            receiver_id,
            class_name,
            property,
            &lowered_args,
        )?;
        record_scalar_method_materialized_fallback(
            ctx,
            receiver_id,
            class_name,
            property,
            &fallback,
            "generic_arg",
            None,
        );
        return Ok(fallback);
    }

    if !arg_plans
        .iter()
        .any(|plan| matches!(plan.kind, ScalarMethodArgKind::GuardedI32Local))
    {
        let mut raw_args = Vec::with_capacity(args.len());
        let raw_i32_locals = HashMap::new();
        for arg in args {
            raw_args.push(lower_int32_scalar_arg_fast(ctx, arg, &raw_i32_locals)?);
        }
        return lower_scalar_method_int32_inline_body(
            ctx,
            receiver_id,
            class_name,
            property,
            method,
            &raw_args,
            "exact_receiver_summary",
            vec!["arg_proof=proven_int32".to_string()],
        );
    }

    let guard_values = collect_i32_guard_local_values(ctx, &arg_plans)?;
    let mut guard: Option<String> = None;
    for (_, value) in &guard_values {
        let raw = ctx
            .block()
            .call(I32, "js_typed_i32_arg_guard", &[(DOUBLE, value.as_str())]);
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
    let mut raw_i32_locals = HashMap::new();
    for (id, value) in &guard_values {
        raw_i32_locals.insert(
            *id,
            ctx.block()
                .call(I32, "js_typed_i32_arg_to_raw", &[(DOUBLE, value.as_str())]),
        );
    }
    let mut fast_args = Vec::with_capacity(args.len());
    for arg in args {
        fast_args.push(lower_int32_scalar_arg_fast(ctx, arg, &raw_i32_locals)?);
    }
    let guarded_arg_count = arg_plans
        .iter()
        .filter(|plan| matches!(plan.kind, ScalarMethodArgKind::GuardedI32Local))
        .count();
    let fast_value = lower_scalar_method_int32_inline_body(
        ctx,
        receiver_id,
        class_name,
        property,
        method,
        &fast_args,
        "guarded_numeric_args_fast_path",
        vec![
            "arg_guard=js_typed_i32_arg_guard".to_string(),
            format!("guarded_arg_count={guarded_arg_count}"),
        ],
    )?;
    let after_fast = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = fallback_idx;
    let mut lowered_args = Vec::with_capacity(args.len());
    for arg in args {
        lowered_args.push(lower_expr(ctx, arg)?);
    }
    let fallback_value = lower_materialized_receiver_dispatch(
        ctx,
        receiver_id,
        class_name,
        property,
        &lowered_args,
    )?;
    record_scalar_method_materialized_fallback(
        ctx,
        receiver_id,
        class_name,
        property,
        &fallback_value,
        "arg_guard_failed",
        Some("js_typed_i32_arg_guard"),
    );
    let after_fallback = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(
        DOUBLE,
        &[
            (fast_value.as_str(), after_fast.as_str()),
            (fallback_value.as_str(), after_fallback.as_str()),
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
    if matches!(method.return_type, Type::Int32) {
        return Ok(Some(lower_scalar_replaced_int32_method_call(
            ctx,
            *receiver_id,
            &class_name,
            property,
            &method,
            args,
        )?));
    }
    let arg_plans: Vec<_> = args
        .iter()
        .zip(method.params.iter())
        .map(|(arg, param)| scalar_method_arg_plan(ctx, arg, &param.ty))
        .collect();

    if arg_plans
        .iter()
        .any(|plan| matches!(plan.kind, ScalarMethodArgKind::Generic))
    {
        let mut lowered_args = Vec::with_capacity(args.len());
        for arg in args {
            lowered_args.push(lower_expr(ctx, arg)?);
        }
        let fallback = lower_materialized_receiver_dispatch(
            ctx,
            *receiver_id,
            &class_name,
            property,
            &lowered_args,
        )?;
        record_scalar_method_materialized_fallback(
            ctx,
            *receiver_id,
            &class_name,
            property,
            &fallback,
            "generic_arg",
            None,
        );
        return Ok(Some(fallback));
    }

    if !arg_plans
        .iter()
        .any(|plan| matches!(plan.kind, ScalarMethodArgKind::GuardedF64Expr))
    {
        let mut lowered_args = Vec::with_capacity(args.len());
        for arg in args {
            lowered_args.push(lower_expr(ctx, arg)?);
        }
        return Ok(Some(lower_scalar_method_inline_body(
            ctx,
            *receiver_id,
            &class_name,
            property,
            &method,
            &lowered_args,
            "exact_receiver_summary",
            vec!["arg_proof=proven_numeric".to_string()],
        )?));
    }

    let guard_values = collect_guard_local_values(ctx, &arg_plans)?;
    let mut guard: Option<String> = None;
    for (_, value) in &guard_values {
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
    let mut raw_locals = HashMap::new();
    for (id, value) in &guard_values {
        raw_locals.insert(
            *id,
            ctx.block().call(
                DOUBLE,
                "js_typed_f64_arg_to_raw",
                &[(DOUBLE, value.as_str())],
            ),
        );
    }
    let mut fast_args = Vec::with_capacity(args.len());
    for arg in args {
        fast_args.push(lower_guarded_numeric_arg_fast(ctx, arg, &raw_locals)?);
    }
    let guarded_arg_count = arg_plans
        .iter()
        .filter(|plan| matches!(plan.kind, ScalarMethodArgKind::GuardedF64Expr))
        .count();
    let guard_note = if arg_plans.iter().any(|plan| plan.expression_guard) {
        "public_numeric_expr"
    } else {
        "js_typed_f64_arg_guard"
    };
    let fast_value = lower_scalar_method_inline_body(
        ctx,
        *receiver_id,
        &class_name,
        property,
        &method,
        &fast_args,
        "guarded_numeric_args_fast_path",
        vec![
            format!("arg_guard={guard_note}"),
            format!("guarded_arg_count={guarded_arg_count}"),
        ],
    )?;
    let after_fast = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = fallback_idx;
    let mut lowered_args = Vec::with_capacity(args.len());
    for arg in args {
        lowered_args.push(lower_expr(ctx, arg)?);
    }
    let fallback_value = lower_materialized_receiver_dispatch(
        ctx,
        *receiver_id,
        &class_name,
        property,
        &lowered_args,
    )?;
    record_scalar_method_materialized_fallback(
        ctx,
        *receiver_id,
        &class_name,
        property,
        &fallback_value,
        "arg_guard_failed",
        Some(guard_note),
    );
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
