//! A compiler-nominated completed literal layout and declared own callee.
//! No site state: the complete layout word and the live value prove the call.
use crate::expr::receiver_range::{emit_field_ptr, emit_fused_receiver_test, emit_handle};
use crate::expr::{lower_expr, FnCtx};
use crate::rooting::{any_operand_may_collect, open_rooted_group, Repr};
use crate::types::{DOUBLE, I1, I16, I32, I64, I8, PTR};
use anyhow::Result;
use perry_hir::Expr;

pub(crate) fn try_lower(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    offset: u32,
) -> Result<Option<String>> {
    if !crate::expr::method_site::method_site_enabled(ctx, property, args.len()) {
        return Ok(None);
    }
    let Some(candidate) = crate::codegen::static_constfn::constructed_object(ctx, object) else {
        return Ok(None);
    };
    let Some(method) = candidate.methods.iter().find(|m| m.name == property) else {
        return Ok(None);
    };
    let Some(layout) = crate::codegen::static_shape_ids::static_content_word(&candidate.shape)
    else {
        return Ok(None);
    };
    ctx.pending_declares.push((
        method.target.clone(),
        DOUBLE,
        crate::expr::body_call::js_body_param_types(method.param_count),
    ));

    let mut roots = open_rooted_group(args.len() + 2);
    let recv = lower_expr(ctx, object)?;
    // The generic lookup can run a getter and collect, even with inert args.
    let owner = roots.adopt_emitted(ctx, Repr::Boxed, &recv, true);
    let key_index = ctx.strings.intern(property);
    let dispatch = ctx.strings.static_dispatch_global(key_index);
    let method_id = crate::strings::emit_static_dispatch_id(ctx.block(), &dispatch);
    let shape_idx = ctx.new_block("completed.layout");
    let slot_idx = ctx.new_block("completed.slot");
    let lookup_idx = ctx.new_block("completed.generic_lookup");
    let selected_idx = ctx.new_block("completed.selected");
    let shape_l = ctx.block_label(shape_idx);
    let slot_l = ctx.block_label(slot_idx);
    let lookup_l = ctx.block_label(lookup_idx);
    let selected_l = ctx.block_label(selected_idx);
    let biased = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(&recv);
        let tested = emit_fused_receiver_test(blk, &bits);
        blk.cond_br(&tested.is_object_pointer, &shape_l, &lookup_l);
        tested.biased
    };
    ctx.current_block = shape_idx;
    {
        let blk = ctx.block();
        let tp = emit_field_ptr(blk, &biased, -(crate::runtime_abi::GC_HEADER_SIZE as i64));
        let flags = blk.load(I16, &tp);
        let mask = (0xffu16 | (u16::from(crate::runtime_abi::GC_FLAG_FORWARDED) << 8)).to_string();
        let kind = blk.and(I16, &flags, &mask);
        let ordinary = blk.icmp_eq(I16, &kind, &crate::runtime_abi::GC_TYPE_OBJECT.to_string());
        let wp = emit_field_ptr(blk, &biased, 4);
        // Full, unmasked equality excludes getters, holes and overflow states.
        let word = blk.load(I32, &wp);
        let same = blk.icmp_eq(I32, &word, &layout.to_string());
        let valid = blk.and(I1, &ordinary, &same);
        blk.cond_br(&valid, &slot_l, &lookup_l);
    }
    ctx.current_block = slot_idx;
    let value = if method.field_index < candidate.shape.live {
        let header = crate::target_layout::object_header_size_bytes(ctx.target_triple) as i64;
        let ptr = emit_field_ptr(
            ctx.block(),
            &biased,
            header + 8 * i64::from(method.field_index),
        );
        ctx.block().load(DOUBLE, &ptr)
    } else {
        // ObjectMeta.spill is a traced owner edge, not a side table. Equality
        // proves this absolute slot has storage (including stored undefined).
        let meta_offset =
            crate::target_layout::object_meta_slot_offset_bytes(ctx.target_triple) as i64;
        let mp = emit_field_ptr(ctx.block(), &biased, meta_offset);
        let meta = ctx.block().load(PTR, &mp);
        let sp = ctx.block().gep(
            I8,
            &meta,
            &[(
                I64,
                &crate::target_layout::OBJECT_META_SPILL_OFFSET_BYTES.to_string(),
            )],
        );
        let spill = ctx.block().load(PTR, &sp);
        let header = crate::target_layout::ARRAY_HEADER_SIZE_BYTES;
        let at = header + 8 * u64::from(method.field_index);
        let ptr = ctx.block().gep(I8, &spill, &[(I64, &at.to_string())]);
        ctx.block().load(DOUBLE, &ptr)
    };
    let slot_end = ctx.block().label.clone();
    ctx.block().br(&selected_l);
    ctx.current_block = lookup_idx;
    crate::expr::calls::emit_call_location_at(ctx, offset);
    let recv = roots.reread_emitted(ctx, owner);
    let looked =
        crate::expr::method_site::emit_method_site_lookup(ctx, property, &recv, &method_id, &[]);
    let lookup_end = ctx.block().label.clone();
    ctx.block().br(&selected_l);
    ctx.current_block = selected_idx;
    let value = ctx
        .block()
        .phi(DOUBLE, &[(&value, &slot_end), (&looked.value, &lookup_end)]);
    let from_slot = ctx
        .block()
        .phi(I1, &[("true", &slot_end), ("false", &lookup_end)]);
    // [[Get]] precedes the arguments. Keep its selected value through any
    // reassignment, deletion, getter installation or collection in an arg.
    let collects = any_operand_may_collect(ctx, args.iter());
    let callee = roots.adopt_emitted(ctx, Repr::Boxed, &value, collects);
    for (index, arg) in args.iter().enumerate() {
        let collects = any_operand_may_collect(ctx, args[index + 1..].iter());
        roots.lower(ctx, arg, collects)?;
    }
    let lowered = roots.reread_all(ctx)?;
    let recv = roots.reread_emitted(ctx, owner);
    let value = roots.reread_emitted(ctx, callee);
    let identity_idx = ctx.new_block("completed.identity");
    let direct_idx = ctx.new_block("completed.direct");
    let generic_idx = ctx.new_block("completed.generic_call");
    let merge_idx = ctx.new_block("completed.merge");
    let identity_l = ctx.block_label(identity_idx);
    let direct_l = ctx.block_label(direct_idx);
    let generic_l = ctx.block_label(generic_idx);
    let merge_l = ctx.block_label(merge_idx);
    ctx.block().cond_br(&from_slot, &identity_l, &generic_l);
    ctx.current_block = identity_idx;
    // A module-function wrapper has one canonical capture-free FuncRef value.
    // The image-owned info identity proves that declared callee without
    // materializing a function value (which would query the singleton cache).
    let kind_idx = ctx.new_block("completed.callee_kind");
    let info_idx = ctx.new_block("completed.callee_info");
    let kind_l = ctx.block_label(kind_idx);
    let info_l = ctx.block_label(info_idx);
    let callee_bias = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(&value);
        let tested = emit_fused_receiver_test(blk, &bits);
        blk.cond_br(&tested.is_object_pointer, &kind_l, &generic_l);
        tested.biased
    };
    ctx.current_block = kind_idx;
    {
        let blk = ctx.block();
        let tp = emit_field_ptr(
            blk,
            &callee_bias,
            -(crate::runtime_abi::GC_HEADER_SIZE as i64),
        );
        let flags = blk.load(I16, &tp);
        let mask = (0xffu16 | (u16::from(crate::runtime_abi::GC_FLAG_FORWARDED) << 8)).to_string();
        let kind = blk.and(I16, &flags, &mask);
        let valid = blk.icmp_eq(I16, &kind, &crate::runtime_abi::GC_TYPE_CLOSURE.to_string());
        blk.cond_br(&valid, &info_l, &generic_l);
    }
    ctx.current_block = info_idx;
    let expected = ctx.block().fn_info_ref(&method.target);
    let ip = emit_field_ptr(
        ctx.block(),
        &callee_bias,
        crate::runtime_abi::CLOSURE_INFO_OFFSET as i64,
    );
    let actual = ctx.block().load(PTR, &ip);
    let same = ctx.block().icmp_eq(PTR, &actual, &expected);
    let closure = emit_handle(ctx.block(), &callee_bias);
    ctx.block().cond_br(&same, &direct_l, &generic_l);
    ctx.current_block = direct_idx;
    let undefined = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let mut call_args: Vec<_> = lowered.iter().take(method.param_count).cloned().collect();
    call_args.resize(method.param_count, undefined);
    let recv_bits = ctx.block().bitcast_double_to_i64(&recv);
    let direct = crate::expr::body_call::emit_js_body_call(
        ctx.block(),
        crate::expr::body_call::JsBody::Symbol(&method.target),
        &closure,
        &recv_bits,
        &call_args,
    );
    let direct_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = generic_idx;
    // An argument may have replaced the debug-only pending call location.
    crate::expr::calls::emit_call_location_at(ctx, offset);
    let selected = crate::expr::method_site::SiteLookup {
        value: value.clone(),
        lanes: Vec::new(),
        property: property.into(),
    };
    let generic = crate::expr::method_site::emit_method_site_call(
        ctx,
        &selected,
        &recv,
        &value,
        &lowered,
        "0",
        &method_id,
        |ctx| {
            if lowered.is_empty() {
                return ("null".into(), "0".into());
            }
            let buf = ctx.func.alloca_entry_array(DOUBLE, lowered.len());
            for (i, value) in lowered.iter().enumerate() {
                let ptr = ctx.block().gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
                ctx.block().store(DOUBLE, value, &ptr);
            }
            (buf, lowered.len().to_string())
        },
    );
    let generic_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = merge_idx;
    let result = ctx
        .block()
        .phi(DOUBLE, &[(&direct, &direct_end), (&generic, &generic_end)]);
    roots.release(ctx);
    Ok(Some(result))
}
