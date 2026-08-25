//! Guarded direct calls for stable imported object-literal own methods (#8775).

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, unbox_to_i64, FnCtx};
use crate::native_value::LoweredValue;
use crate::rooting::{any_operand_may_collect, open_rooted_group, Repr};
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

const POINTER_TAG_HI16: &str = "32765"; // 0x7FFD
const GC_OBJECT_METHOD_GUARD_MASK_I32: &str = "142639359"; // 0x0880_80ff
const GC_TYPE_OBJECT: &str = "2";

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

/// Emit a monomorphic own-method cache that can relearn an append-only shape
/// successor after an adapter's `setup()` adds state fields. `entry_idx` is
/// reached only after exact exported-object identity matched. On return the
/// current block is the direct arm and the result is the validated raw closure
/// handle to pass to the producer body.
#[allow(clippy::too_many_arguments)]
fn emit_cached_own_method_guard(
    ctx: &mut FnCtx<'_>,
    entry_idx: usize,
    recv: &str,
    recv_bits: &str,
    expected_class_id: u32,
    field_index: u32,
    property: &str,
    closure_symbol: &str,
    miss_label: &str,
) -> String {
    let key_idx = ctx.strings.intern(property);
    let key_entry = ctx.strings.entry(key_idx);
    let bytes_global = format!("@{}", key_entry.bytes_global);
    let name_len = key_entry.byte_len.to_string();

    let cache_site = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache_name = crate::expr::inline_cache_global_name(ctx, cache_site);
    ctx.ic_globals.push(cache_name.clone());
    let cache_ref = format!("@{cache_name}");

    let deref_idx = ctx.new_block("object_method_cache.deref");
    let fast_idx = ctx.new_block("object_method_cache.fast");
    let cold_idx = ctx.new_block("object_method_cache.revalidate");
    let direct_idx = ctx.new_block("object_method_cache.direct");
    let deref_label = ctx.block_label(deref_idx);
    let fast_label = ctx.block_label(fast_idx);
    let cold_label = ctx.block_label(cold_idx);
    let direct_label = ctx.block_label(direct_idx);

    ctx.current_block = entry_idx;
    let heap_floor =
        crate::target_layout::heap_addr_lower_bound_inclusive(ctx.target_triple).to_string();
    let heap_ceiling =
        crate::target_layout::heap_addr_upper_bound_exclusive(ctx.target_triple).to_string();
    let recv_handle = ctx
        .block()
        .and(I64, recv_bits, crate::nanbox::POINTER_MASK_I64);
    let tag = ctx.block().lshr(I64, recv_bits, "48");
    let tagged = ctx.block().icmp_eq(I64, &tag, POINTER_TAG_HI16);
    let above_floor = ctx.block().icmp_uge(I64, &recv_handle, &heap_floor);
    let below_ceiling = ctx.block().icmp_ult(I64, &recv_handle, &heap_ceiling);
    let in_range = ctx.block().and(I1, &above_floor, &below_ceiling);
    let safe = ctx.block().and(I1, &tagged, &in_range);
    ctx.block().cond_br(&safe, &deref_label, miss_label);

    ctx.current_block = deref_idx;
    let object_ptr = ctx.block().inttoptr(I64, &recv_handle);
    let gc_header_ptr = ctx.block().gep(I8, &object_ptr, &[(I64, "-8")]);
    let gc_header = ctx.block().load(I32, &gc_header_ptr);
    let guarded_gc_bits = ctx
        .block()
        .and(I32, &gc_header, GC_OBJECT_METHOD_GUARD_MASK_I32);
    let gc_header_ok = ctx.block().icmp_eq(I32, &guarded_gc_bits, GC_TYPE_OBJECT);
    let live_class_shape = ctx.block().load(I64, &object_ptr);
    let cache_token_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "0")]);
    let cached_class_shape = ctx.block().load(I64, &cache_token_ptr);
    let cache_populated = ctx.block().icmp_ne(I64, &cached_class_shape, "0");
    let shape_matches = ctx
        .block()
        .icmp_eq(I64, &live_class_shape, &cached_class_shape);
    let cache_hit = ctx.block().and(I1, &gc_header_ok, &cache_populated);
    let cache_hit = ctx.block().and(I1, &cache_hit, &shape_matches);
    ctx.block().cond_br(&cache_hit, &fast_label, &cold_label);

    ctx.current_block = fast_idx;
    let header_skip = crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let fields = ctx.block().gep(I8, &object_ptr, &[(I64, &header_skip)]);
    let slot = ctx
        .block()
        .gep(DOUBLE, &fields, &[(I64, &field_index.to_string())]);
    let closure_value = ctx.block().load(DOUBLE, &slot);
    let guard = ctx.block().call(
        I32,
        "js_closure_exact_func_guard",
        &[
            (DOUBLE, &closure_value),
            (PTR, &format!("@{closure_symbol}")),
        ],
    );
    let guard_passes = ctx.block().icmp_ne(I32, &guard, "0");
    let fast_handle = unbox_to_i64(ctx.block(), &closure_value);
    let fast_end = ctx.block().label.clone();
    ctx.block()
        .cond_br(&guard_passes, &direct_label, &cold_label);

    ctx.current_block = cold_idx;
    let cold_handle = ctx.block().call(
        I64,
        "js_object_own_method_cache_miss",
        &[
            (DOUBLE, recv),
            (I32, &expected_class_id.to_string()),
            (I32, &field_index.to_string()),
            (PTR, &bytes_global),
            (I64, &name_len),
            (PTR, &format!("@{closure_symbol}")),
            (PTR, &cache_token_ptr),
        ],
    );
    let cold_passes = ctx.block().icmp_ne(I64, &cold_handle, "0");
    let cold_end = ctx.block().label.clone();
    ctx.block().cond_br(&cold_passes, &direct_label, miss_label);

    ctx.current_block = direct_idx;
    ctx.block().phi(
        I64,
        &[
            (fast_handle.as_str(), fast_end.as_str()),
            (cold_handle.as_str(), cold_end.as_str()),
        ],
    )
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
    let dispatch_global = ctx.strings.static_dispatch_global(key_index);
    let closure_symbol = method.target.clone();
    let mut closure_params = Vec::with_capacity(method.param_count + 1);
    closure_params.push(I64);
    closure_params.extend(std::iter::repeat_n(DOUBLE, method.param_count));
    ctx.pending_declares
        .push((closure_symbol.clone(), DOUBLE, closure_params));

    let cache_idx = ctx.new_block("imported_object.cache_guard");
    let fallback_idx = ctx.new_block("imported_object.fallback");
    let merge_idx = ctx.new_block("imported_object.merge");
    let cache_label = ctx.block_label(cache_idx);
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
        .cond_br(&receiver_matches, &cache_label, &fallback_label);

    let closure_handle = emit_cached_own_method_guard(
        ctx,
        cache_idx,
        &recv,
        &recv_bits,
        expected_class_id,
        method.field_index,
        property,
        &closure_symbol,
        &fallback_label,
    );
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
            "guards=receiver_identity,live_shape_cache,own_key_slot,function_identity".to_string(),
            "append_only_shape_successors=revalidated_and_cached".to_string(),
            "generic_dispatch_fallback=js_native_call_method_by_id".to_string(),
        ],
    );
    Ok(Some(result))
}

/// Select a stable exported object-literal method from an otherwise dynamic
/// local receiver. This is needed when the receiver crosses a module boundary
/// through a parameter (`suite.perform(library)`): there is no import binding
/// in the suite module, but whole-program producer metadata still gives us a
/// finite set of exact identities to guard.
pub(super) fn try_lower_dynamic_object_method_call(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
) -> Result<Option<String>> {
    const MAX_ARMS: usize = 8;

    // Keep this reverse-flow slice deliberately narrow. Known class, builtin,
    // and direct-import receivers have already had their more specific routes.
    if !matches!(object, Expr::LocalGet(_)) || receiver_binding(ctx, object).is_some() {
        return Ok(None);
    }
    let Some(by_name) = ctx.object_literal_method_candidates.get(property) else {
        return Ok(None);
    };
    let candidates: Vec<_> = by_name
        .iter()
        .filter(|candidate| candidate.method.param_count == args.len())
        .cloned()
        .collect();
    if candidates.is_empty() || candidates.len() > MAX_ARMS {
        return Ok(None);
    }

    // Receiver, then arguments, exactly once. All values remain rooted across
    // every guard arm, the direct closure body, and the generic fallback.
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
    let recv_bits = ctx.block().bitcast_double_to_i64(&recv);

    let key_index = ctx.strings.intern(property);
    let dispatch_global = ctx.strings.static_dispatch_global(key_index);
    let fallback_idx = ctx.new_block("object_candidate.fallback");
    let merge_idx = ctx.new_block("object_candidate.merge");
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    let mut direct_results = Vec::with_capacity(candidates.len() + 1);

    for (index, candidate) in candidates.iter().enumerate() {
        let cache_idx = ctx.new_block("object_candidate.cache_guard");
        let next_idx =
            (index + 1 < candidates.len()).then(|| ctx.new_block("object_candidate.next"));
        let cache_label = ctx.block_label(cache_idx);
        let miss_label = next_idx
            .map(|block| ctx.block_label(block))
            .unwrap_or_else(|| fallback_label.clone());

        let source_global = format!(
            "@perry_global_{}__{}",
            candidate.source_prefix, candidate.source_global_id
        );
        let expected_receiver = ctx.block().load(DOUBLE, &source_global);
        let expected_bits = ctx.block().bitcast_double_to_i64(&expected_receiver);
        let receiver_matches = ctx.block().icmp_eq(I64, &recv_bits, &expected_bits);
        ctx.block()
            .cond_br(&receiver_matches, &cache_label, &miss_label);

        let closure_symbol = candidate.method.target.clone();
        let mut closure_params = Vec::with_capacity(candidate.method.param_count + 1);
        closure_params.push(I64);
        closure_params.extend(std::iter::repeat_n(DOUBLE, candidate.method.param_count));
        ctx.pending_declares
            .push((closure_symbol.clone(), DOUBLE, closure_params));
        let closure_handle = emit_cached_own_method_guard(
            ctx,
            cache_idx,
            &recv,
            &recv_bits,
            candidate.class_id,
            candidate.method.field_index,
            property,
            &closure_symbol,
            &miss_label,
        );
        let mut direct_args: Vec<(crate::types::LlvmType, &str)> =
            Vec::with_capacity(lowered_args.len() + 1);
        direct_args.push((I64, &closure_handle));
        direct_args.extend(lowered_args.iter().map(|arg| (DOUBLE, arg.as_str())));
        let direct_value = ctx.block().call(DOUBLE, &closure_symbol, &direct_args);
        let direct_end = ctx.block().label.clone();
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }
        direct_results.push((direct_value, direct_end));

        if let Some(next_idx) = next_idx {
            ctx.current_block = next_idx;
        }
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
    direct_results.push((fallback_value, fallback_end));

    ctx.current_block = merge_idx;
    let incoming: Vec<_> = direct_results
        .iter()
        .map(|(value, block)| (value.as_str(), block.as_str()))
        .collect();
    let result = ctx.block().phi(DOUBLE, &incoming);
    roots.release(ctx);
    let selected = candidates
        .iter()
        .map(|candidate| {
            format!(
                "{}#{}:{}",
                candidate.source_prefix, candidate.source_global_id, candidate.method.func_id
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    ctx.record_lowered_value(
        "MethodCall",
        None,
        "whole_program_object_literal_method_direct_call",
        &LoweredValue::js_value(result.clone()),
        None,
        None,
        None,
        false,
        false,
        vec![
            "receiver_provenance=dynamic_local_with_producer_candidates".to_string(),
            format!("method={property}"),
            format!("selected_method_identities={selected}"),
            "guards=receiver_identity,live_shape_cache,own_key_slot,function_identity".to_string(),
            "append_only_shape_successors=revalidated_and_cached".to_string(),
            "generic_dispatch_fallback=js_native_call_method_by_id".to_string(),
        ],
    );
    Ok(Some(result))
}
