//! Guarded direct calls for stable imported object-literal own methods (#8775).

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{
    emit_typed_feedback_register_site, lower_expr, unbox_to_i64, FnCtx, TypedFeedbackContract,
    TypedFeedbackKind,
};
use crate::native_value::LoweredValue;
use crate::rooting::{any_operand_may_collect, open_rooted_group, Repr};
use crate::types::{DOUBLE, I32, I64, I8, PTR};

fn receiver_binding(ctx: &FnCtx<'_>, object: &Expr) -> Option<String> {
    match object {
        Expr::ExternFuncRef { name, .. } if ctx.imported_object_literals.contains_key(name) => {
            Some(name.clone())
        }
        Expr::LocalGet(id) => ctx.local_imported_object_aliases.get(id).cloned(),
        _ => None,
    }
}

fn spill_args(ctx: &mut FnCtx<'_>, args: &[String]) -> (String, String) {
    if args.is_empty() {
        return ("null".to_string(), "0".to_string());
    }
    let buf = ctx.func.alloca_entry_array(DOUBLE, args.len());
    for (index, value) in args.iter().enumerate() {
        let slot = ctx.block().gep(DOUBLE, &buf, &[(I64, &index.to_string())]);
        ctx.block().store(DOUBLE, value, &slot);
    }
    (buf, args.len().to_string())
}

pub(super) fn try_lower_imported_object_method_call(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
) -> Result<Option<String>> {
    let Some(binding) = receiver_binding(ctx, object) else {
        return Ok(None);
    };
    let Some(capability) = ctx.imported_object_literals.get(&binding).cloned() else {
        return Ok(None);
    };
    let Some(method) = capability
        .methods
        .iter()
        .find(|method| method.name == property && method.param_count == args.len())
        .cloned()
    else {
        // Function-valued properties, accessors, and arity-changing methods are
        // intentionally outside the capability. Let the universal dispatcher
        // preserve their dynamic receiver/call semantics.
        return Ok(None);
    };
    let Some(expected_class_id) = ctx.class_ids.get(&capability.receiver_class_name).copied()
    else {
        return Ok(None);
    };
    let Some(keys_global) = ctx
        .class_keys_globals
        .get(&capability.receiver_class_name)
        .cloned()
    else {
        return Ok(None);
    };

    // JavaScript evaluates the receiver before arguments. Keep that value (and
    // each argument) rooted through both branches, then run all guards after
    // argument evaluation so a mutating argument cannot slip past the proof.
    let mut roots = open_rooted_group(args.len() + 1);
    let recv = lower_expr(ctx, object)?;
    let receiver_collects = any_operand_may_collect(ctx, args.iter());
    let receiver_root = roots.adopt_emitted(ctx, Repr::Boxed, &recv, receiver_collects);
    for (index, arg) in args.iter().enumerate() {
        let collects = any_operand_may_collect(ctx, args[index + 1..].iter());
        roots.lower(ctx, arg, collects)?;
    }
    let recv = roots.reread_emitted(ctx, receiver_root);
    let lowered_args = roots.reread_all(ctx)?;

    let key_index = ctx.strings.intern(property);
    let key_entry = ctx.strings.entry(key_index);
    let method_guard_slot = (key_entry.dispatch_hash & 0xffff).to_string();
    let dispatch_global = ctx.strings.static_dispatch_global(key_index);
    let expected_shape_id =
        crate::typed_shape::load_class_shape_id(ctx, &capability.receiver_class_name, &keys_global);
    let closure_symbol = format!(
        "perry_closure_{}__{}",
        capability.source_prefix, method.func_id
    );
    let mut closure_params = Vec::with_capacity(method.param_count + 1);
    closure_params.push(I64);
    closure_params.extend(std::iter::repeat_n(DOUBLE, method.param_count));
    ctx.pending_declares
        .push((closure_symbol.clone(), DOUBLE, closure_params));

    let shape_idx = ctx.new_block("imported_object.shape_guard");
    let method_idx = ctx.new_block("imported_object.method_guard");
    let direct_idx = ctx.new_block("imported_object.direct");
    let fallback_idx = ctx.new_block("imported_object.fallback");
    let merge_idx = ctx.new_block("imported_object.merge");
    let shape_label = ctx.block_label(shape_idx);
    let method_label = ctx.block_label(method_idx);
    let direct_label = ctx.block_label(direct_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);

    // Exact binding identity is separate from shape: another object may share
    // the same anonymous layout and even the same method closure function.
    let source_global = format!(
        "@perry_global_{}__{}",
        capability.source_prefix, capability.source_global_id
    );
    let expected_receiver = ctx.block().load(DOUBLE, &source_global);
    let recv_bits = ctx.block().bitcast_double_to_i64(&recv);
    let expected_bits = ctx.block().bitcast_double_to_i64(&expected_receiver);
    let receiver_matches = ctx.block().icmp_eq(I64, &recv_bits, &expected_bits);
    ctx.block()
        .cond_br(&receiver_matches, &shape_label, &fallback_label);

    ctx.current_block = shape_idx;
    crate::lower_call::method_override::emit_inline_direct_method_shape_guard(
        ctx,
        &recv,
        &expected_class_id.to_string(),
        &expected_shape_id,
        &method_guard_slot,
        &method_label,
        &fallback_label,
    );

    // The exact shape proves the own data slot. Load it directly, then validate
    // the live closure's underlying function identity. Replacement, deletion,
    // accessors, and bound/arrow substitutes all fail one of these guards.
    ctx.current_block = method_idx;
    let closure_value = {
        let header_skip =
            crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
        let blk = ctx.block();
        let recv_handle = blk.and(I64, &recv_bits, crate::nanbox::POINTER_MASK_I64);
        let object_ptr = blk.inttoptr(I64, &recv_handle);
        let fields = blk.gep(I8, &object_ptr, &[(I64, &header_skip)]);
        let slot = blk.gep(DOUBLE, &fields, &[(I64, &method.field_index.to_string())]);
        blk.load(DOUBLE, &slot)
    };
    let site_id = emit_typed_feedback_register_site(
        ctx,
        TypedFeedbackKind::ClosureCall,
        &format!("imported-object:{binding}.{property}"),
        TypedFeedbackContract::closure_direct_call(),
    );
    let arity = method.param_count.to_string();
    let guard = ctx.block().call(
        I32,
        "js_typed_feedback_closure_direct_call_guard",
        &[
            (I64, &site_id),
            (DOUBLE, &closure_value),
            (PTR, &format!("@{closure_symbol}")),
            (I32, &arity),
            (I32, &arity),
        ],
    );
    let guard_passes = ctx.block().icmp_ne(I32, &guard, "0");
    ctx.block()
        .cond_br(&guard_passes, &direct_label, &fallback_label);

    ctx.current_block = direct_idx;
    let closure_handle = unbox_to_i64(ctx.block(), &closure_value);
    let mut direct_args: Vec<(crate::types::LlvmType, &str)> =
        Vec::with_capacity(lowered_args.len() + 1);
    direct_args.push((I64, &closure_handle));
    direct_args.extend(lowered_args.iter().map(|arg| (DOUBLE, arg.as_str())));
    let direct_value = ctx.block().call(DOUBLE, &closure_symbol, &direct_args);
    let direct_end = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = fallback_idx;
    let method_id = crate::strings::emit_static_dispatch_id(ctx.block(), &dispatch_global);
    let (args_ptr, args_len) = spill_args(ctx, &lowered_args);
    crate::expr::calls::emit_call_location_at(ctx, call_byte_offset);
    let fallback_value = ctx.block().call(
        DOUBLE,
        "js_native_call_method_by_id",
        &[
            (DOUBLE, &recv),
            (I64, &method_id),
            (PTR, &args_ptr),
            (I64, &args_len),
        ],
    );
    let fallback_end = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    let result = ctx.block().phi(
        DOUBLE,
        &[
            (direct_value.as_str(), direct_end.as_str()),
            (fallback_value.as_str(), fallback_end.as_str()),
        ],
    );
    roots.release(ctx);
    ctx.record_lowered_value(
        "MethodCall",
        None,
        "imported_object_literal_method_direct_call",
        &LoweredValue::js_value(result.clone()),
        None,
        None,
        None,
        false,
        false,
        vec![
            "receiver_provenance=imported_object_literal_metadata".to_string(),
            format!("source_export={}", capability.source_export_name),
            format!("receiver_class={}", capability.receiver_class_name),
            format!("method={property}"),
            format!("selected_method_identity={closure_symbol}"),
            format!("field_index={}", method.field_index),
            "guards=receiver_identity,exact_shape,own_data_slot,function_identity".to_string(),
            "generic_dispatch_fallback=js_native_call_method_by_id".to_string(),
        ],
    );
    Ok(Some(result))
}
