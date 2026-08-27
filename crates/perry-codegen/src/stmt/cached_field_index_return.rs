//! Guarded fast return for `if (!owner.table[i]) { ... } return owner.table[i]`.
//!
//! The source performs the indexed read twice on the already-populated path.
//! Reusing the first result is not generally valid: either property access can
//! invoke a getter/Proxy and the second observation is required. This lowering
//! therefore adds only a speculative *proof* path. It returns early solely
//! after proving an own shape-cached data field, an ordinary dense Array, an
//! in-bounds data slot, and a truthy result. Every failure enters the original
//! statements unchanged.

use anyhow::Result;
use perry_hir::{Expr, Stmt, UnaryOp};

use crate::expr::{lower_expr, FnCtx, PropertyGetIcOverride};
use crate::lower_conditional::lower_truthy;
use crate::types::{DOUBLE, I1, I16, I32, I64, I8};

struct Candidate<'a> {
    base_local_id: u32,
    property: &'a str,
    index_local_id: u32,
    access: &'a Expr,
}

fn field_index_access(expr: &Expr) -> Option<(u32, &str, u32)> {
    let Expr::IndexGet { object, index } = expr else {
        return None;
    };
    let Expr::PropertyGet {
        object: base,
        property,
        ..
    } = object.as_ref()
    else {
        return None;
    };
    let Expr::LocalGet(base_local_id) = base.as_ref() else {
        return None;
    };
    let Expr::LocalGet(index_local_id) = index.as_ref() else {
        return None;
    };
    Some((*base_local_id, property.as_str(), *index_local_id))
}

fn match_candidate(stmts: &[Stmt]) -> Option<Candidate<'_>> {
    let (
        Stmt::If {
            condition,
            else_branch: None,
            ..
        },
        Stmt::Return(Some(returned)),
    ) = (stmts.first()?, stmts.get(1)?)
    else {
        return None;
    };
    let Expr::Unary {
        op: UnaryOp::Not,
        operand,
    } = condition
    else {
        return None;
    };
    let (base_local_id, property, index_local_id) = field_index_access(operand)?;
    let (return_base, return_property, return_index) = field_index_access(returned)?;
    if (base_local_id, property, index_local_id) != (return_base, return_property, return_index) {
        return None;
    }
    Some(Candidate {
        base_local_id,
        property,
        index_local_id,
        access: operand,
    })
}

fn allocate_shared_cache(ctx: &mut FnCtx<'_>, candidate: &Candidate<'_>) -> String {
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache_name = crate::expr::inline_cache_global_name(ctx, site_id);
    ctx.pending_declares
        .push((format!("__ic_decl_{site_id}"), DOUBLE, vec![]));
    ctx.ic_globals.push(cache_name.clone());
    ctx.property_get_ic_override = Some(PropertyGetIcOverride {
        base_local_id: candidate.base_local_id,
        property: candidate.property.to_string(),
        cache_name: cache_name.clone(),
    });
    cache_name
}

/// Emit the speculative early-return path and leave `ctx.current_block` on the
/// normal fallback block. The caller then lowers the original statements.
pub(super) fn try_emit_cached_field_index_return(
    ctx: &mut FnCtx<'_>,
    stmts: &[Stmt],
) -> Result<bool> {
    let Some(candidate) = match_candidate(stmts) else {
        return Ok(false);
    };
    let Some(index_slot) = ctx
        .i32_counter_slots
        .get(&candidate.index_local_id)
        .cloned()
    else {
        return Ok(false);
    };
    if !ctx
        .nonnegative_integer_locals
        .contains(&candidate.index_local_id)
        || ctx.property_get_ic_override.is_some()
        || ctx.is_async_fn
        || ctx.try_depth != 0
        || !ctx.inline_ctor_return.is_empty()
        || ctx.shared_super_scope_active
    {
        return Ok(false);
    }

    let cache_name = allocate_shared_cache(ctx, &candidate);
    let cache_ref = format!("@{cache_name}");
    let base_box = lower_expr(ctx, &Expr::LocalGet(candidate.base_local_id))?;
    let index_i32 = ctx.block().load(I32, &index_slot);

    let object_header_idx = ctx.new_block("cached_field_index.object_header");
    let exact_or_prefix_idx = ctx.new_block("cached_field_index.exact_or_prefix");
    let exact_token_idx = ctx.new_block("cached_field_index.exact_token");
    let prefix_meta_idx = ctx.new_block("cached_field_index.prefix_meta");
    let prefix_token_idx = ctx.new_block("cached_field_index.prefix_token");
    let field_load_idx = ctx.new_block("cached_field_index.field_load");
    let array_header_idx = ctx.new_block("cached_field_index.array_header");
    let array_load_idx = ctx.new_block("cached_field_index.array_load");
    let truthy_idx = ctx.new_block("cached_field_index.truthy");
    let return_idx = ctx.new_block("cached_field_index.return");
    let normal_idx = ctx.new_block("cached_field_index.normal");
    let object_header_label = ctx.block_label(object_header_idx);
    let exact_or_prefix_label = ctx.block_label(exact_or_prefix_idx);
    let exact_token_label = ctx.block_label(exact_token_idx);
    let prefix_meta_label = ctx.block_label(prefix_meta_idx);
    let prefix_token_label = ctx.block_label(prefix_token_idx);
    let field_load_label = ctx.block_label(field_load_idx);
    let array_header_label = ctx.block_label(array_header_idx);
    let array_load_label = ctx.block_label(array_load_idx);
    let truthy_label = ctx.block_label(truthy_idx);
    let return_label = ctx.block_label(return_idx);
    let normal_label = ctx.block_label(normal_idx);

    let tag_mask = crate::nanbox::i64_literal(crate::nanbox::TAG_MASK);
    let object_raw = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(&base_box);
        let raw = blk.and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
        let tag = blk.and(I64, &bits, &tag_mask);
        let is_pointer = blk.icmp_eq(I64, &tag, crate::nanbox::POINTER_TAG_I64);
        let above_handles = blk.icmp_ugt(I64, &raw, "1048575");
        let eligible = blk.and(I1, &is_pointer, &above_handles);
        blk.cond_br(&eligible, &object_header_label, &normal_label);
        raw
    };

    ctx.current_block = object_header_idx;
    let gc_type_addr = ctx.block().sub(I64, &object_raw, "8");
    let gc_type_ptr = ctx.block().inttoptr(I64, &gc_type_addr);
    let gc_type = ctx.block().load(I8, &gc_type_ptr);
    let is_object = ctx.block().icmp_eq(I8, &gc_type, "2");
    let gc_flags_addr = ctx.block().sub(I64, &object_raw, "7");
    let gc_flags_ptr = ctx.block().inttoptr(I64, &gc_flags_addr);
    let gc_flags = ctx.block().load(I8, &gc_flags_ptr);
    let forwarded_bits = ctx.block().and(I8, &gc_flags, "128");
    let not_forwarded = ctx.block().icmp_eq(I8, &forwarded_bits, "0");
    let object_ok = ctx.block().and(I1, &is_object, &not_forwarded);
    ctx.block()
        .cond_br(&object_ok, &exact_or_prefix_label, &normal_label);

    ctx.current_block = exact_or_prefix_idx;
    let reserved_addr = ctx.block().sub(I64, &object_raw, "6");
    let reserved_ptr = ctx.block().inttoptr(I64, &reserved_addr);
    let reserved = ctx.block().load(I16, &reserved_ptr);
    let descriptor_bits = ctx.block().and(I16, &reserved, "2048");
    let no_descriptors = ctx.block().icmp_eq(I16, &descriptor_bits, "0");
    ctx.block()
        .cond_br(&no_descriptors, &exact_token_label, &prefix_meta_label);

    // Descriptor-bearing instances (including Array subclasses with an own
    // `length`) cannot use an exact ShapeId property slot. Send them straight
    // to the data-only named-prefix proof instead of loading a dead shape.
    ctx.current_block = exact_token_idx;
    let shape_addr = ctx.block().add(I64, &object_raw, "4");
    let shape_ptr = ctx.block().inttoptr(I64, &shape_addr);
    let shape_id = ctx.block().load(I32, &shape_ptr);
    let shape_nonzero = ctx.block().icmp_ne(I32, &shape_id, "0");
    let shape_i64 = ctx.block().zext(I32, &shape_id, I64);
    let live_token = ctx.block().or(I64, &shape_i64, "4611686018427387904");
    let cached_token_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "0")]);
    let cached_token = ctx.block().load(I64, &cached_token_ptr);
    let token_matches = ctx.block().icmp_eq(I64, &live_token, &cached_token);
    let exact = ctx.block().and(I1, &shape_nonzero, &token_matches);
    ctx.block()
        .cond_br(&exact, &field_load_label, &prefix_meta_label);

    ctx.current_block = prefix_meta_idx;
    let cached_prefix_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "2")]);
    let cached_prefix = ctx.block().load(I64, &cached_prefix_ptr);
    let prefix_armed = ctx.block().icmp_ne(I64, &cached_prefix, "0");
    let pointer_bytes = if crate::target_layout::target_is_ilp32(ctx.target_triple) {
        4
    } else {
        8
    };
    let meta_offset = (crate::target_layout::object_header_size_bytes(ctx.target_triple)
        - pointer_bytes)
        .to_string();
    let meta_addr = ctx.block().add(I64, &object_raw, &meta_offset);
    let meta_slot = ctx.block().inttoptr(I64, &meta_addr);
    let meta_ty = if pointer_bytes == 4 { I32 } else { I64 };
    let meta_raw = ctx.block().load(meta_ty, &meta_slot);
    let meta = if pointer_bytes == 4 {
        ctx.block().zext(I32, &meta_raw, I64)
    } else {
        meta_raw
    };
    let meta_nonzero = ctx.block().icmp_ne(I64, &meta, "0");
    let can_check_prefix = ctx.block().and(I1, &prefix_armed, &meta_nonzero);
    ctx.block()
        .cond_br(&can_check_prefix, &prefix_token_label, &normal_label);

    ctx.current_block = prefix_token_idx;
    let meta_ptr = ctx.block().inttoptr(I64, &meta);
    let object_prefix_ptr = ctx.block().gep(I64, &meta_ptr, &[(I64, "6")]);
    let object_prefix = ctx.block().load(I64, &object_prefix_ptr);
    let prefix_matches = ctx.block().icmp_eq(I64, &object_prefix, &cached_prefix);
    ctx.block()
        .cond_br(&prefix_matches, &field_load_label, &normal_label);

    ctx.current_block = field_load_idx;
    let cached_slot_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "1")]);
    let cached_slot = ctx.block().load(I64, &cached_slot_ptr);
    let field_offset = ctx.block().shl(I64, &cached_slot, "3");
    let header_size = crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let fields_base = ctx.block().add(I64, &object_raw, &header_size);
    let field_addr = ctx.block().add(I64, &fields_base, &field_offset);
    let field_ptr = ctx.block().inttoptr(I64, &field_addr);
    let array_box = ctx.block().load(DOUBLE, &field_ptr);
    let array_bits = ctx.block().bitcast_double_to_i64(&array_box);
    let array_raw = ctx
        .block()
        .and(I64, &array_bits, crate::nanbox::POINTER_MASK_I64);
    let array_tag = ctx.block().and(I64, &array_bits, &tag_mask);
    let array_is_pointer = ctx
        .block()
        .icmp_eq(I64, &array_tag, crate::nanbox::POINTER_TAG_I64);
    let array_above_handles = ctx.block().icmp_ugt(I64, &array_raw, "1048575");
    let array_address_ok = ctx.block().and(I1, &array_is_pointer, &array_above_handles);
    ctx.block()
        .cond_br(&array_address_ok, &array_header_label, &normal_label);

    ctx.current_block = array_header_idx;
    let array_gc_type_addr = ctx.block().sub(I64, &array_raw, "8");
    let array_gc_type_ptr = ctx.block().inttoptr(I64, &array_gc_type_addr);
    let array_gc_type = ctx.block().load(I8, &array_gc_type_ptr);
    let is_array = ctx.block().icmp_eq(I8, &array_gc_type, "1");
    let array_gc_flags_addr = ctx.block().sub(I64, &array_raw, "7");
    let array_gc_flags_ptr = ctx.block().inttoptr(I64, &array_gc_flags_addr);
    let array_gc_flags = ctx.block().load(I8, &array_gc_flags_ptr);
    let array_forwarded_bits = ctx.block().and(I8, &array_gc_flags, "128");
    let array_not_forwarded = ctx.block().icmp_eq(I8, &array_forwarded_bits, "0");
    let array_reserved_addr = ctx.block().sub(I64, &array_raw, "6");
    let array_reserved_ptr = ctx.block().inttoptr(I64, &array_reserved_addr);
    let array_reserved = ctx.block().load(I16, &array_reserved_ptr);
    let array_descriptor_bits = ctx.block().and(I16, &array_reserved, "1024");
    let array_no_descriptors = ctx.block().icmp_eq(I16, &array_descriptor_bits, "0");
    let invalidated = ctx
        .block()
        .load_volatile(I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
    let default_prototypes = ctx.block().icmp_eq(I8, &invalidated, "0");
    let array_ptr = ctx.block().inttoptr(I64, &array_raw);
    let length = ctx.block().load(I32, &array_ptr);
    let capacity_addr = ctx.block().add(I64, &array_raw, "4");
    let capacity_ptr = ctx.block().inttoptr(I64, &capacity_addr);
    let capacity = ctx.block().load(I32, &capacity_ptr);
    let index_in_bounds = ctx.block().icmp_ult(I32, &index_i32, &length);
    let sane_capacity = ctx.block().icmp_ule(I32, &length, &capacity);
    let array_ok = ctx.block().and(I1, &is_array, &array_not_forwarded);
    let array_ok = ctx.block().and(I1, &array_ok, &array_no_descriptors);
    let array_ok = ctx.block().and(I1, &array_ok, &default_prototypes);
    let array_ok = ctx.block().and(I1, &array_ok, &index_in_bounds);
    let array_ok = ctx.block().and(I1, &array_ok, &sane_capacity);
    ctx.block()
        .cond_br(&array_ok, &array_load_label, &normal_label);

    ctx.current_block = array_load_idx;
    let index_i64 = ctx.block().zext(I32, &index_i32, I64);
    let element_word = ctx.block().add(I64, &index_i64, "1");
    let element_ptr = ctx
        .block()
        .gep_inbounds(I64, &array_ptr, &[(I64, &element_word)]);
    let raw_value = ctx.block().load(DOUBLE, &element_ptr);
    let raw_bits = ctx.block().bitcast_double_to_i64(&raw_value);
    let is_hole = ctx
        .block()
        .icmp_eq(I64, &raw_bits, crate::nanbox::TAG_HOLE_I64);
    let undefined = ctx
        .block()
        .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
    let value = ctx
        .block()
        .select(I1, &is_hole, DOUBLE, &undefined, &raw_value);
    ctx.block().br(&truthy_label);

    ctx.current_block = truthy_idx;
    let is_truthy = lower_truthy(ctx, &value, candidate.access);
    ctx.block()
        .cond_br(&is_truthy, &return_label, &normal_label);

    ctx.current_block = return_idx;
    ctx.block().ret(DOUBLE, &value);

    ctx.current_block = normal_idx;
    Ok(true)
}
