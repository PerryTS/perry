//! Composed inline caches for property reads: the Symbol-then-named-field IC
//! and the object-backed Array-subclass `.length` tier.
//!
//! Child module of `property_get.rs`, split out to stay under the 2,000-line
//! file gate; `use super::*` keeps the parent's private helpers reachable.

use super::*;

/// Fuse `base[provenSymbol].field` into one weak identity/epoch guard followed
/// by one exact ShapeId guard and direct slot load.
///
/// The ordinary Symbol IC already proves that its indexed intermediate value is
/// the current own Symbol data property and invalidates on every Symbol write
/// or completed GC. A second ordinary property PIC currently throws that fact
/// away and repeats receiver tag, GC-header, descriptor, ShapeId, and dispatch
/// classification. This composed site keeps two normal cache records: the
/// Symbol identity/value cache and an ordinary property cache primed by the
/// shared runtime miss handler. A hit still reloads the named field's current
/// bits; it never caches the final value, so `metadata.id = next` is observed
/// without an epoch bump.
pub(super) fn lower_symbol_then_named_property_ic(
    ctx: &mut FnCtx<'_>,
    base: &Expr,
    symbol: &Expr,
    property: &str,
    byte_offset: u32,
) -> Result<String> {
    rooting::with_operands_rooted(ctx, &[base, symbol], |ctx, values| {
        let base_box = ctx.block().call(
            DOUBLE,
            "js_require_object_coercible",
            &[(DOUBLE, values[0].as_str())],
        );
        let symbol_box = values[1].clone();
        crate::expr::calls::emit_call_location_at(ctx, byte_offset);
        let feedback_site_id = emit_typed_feedback_register_site(
            ctx,
            TypedFeedbackKind::PropertyGet,
            property,
            TypedFeedbackContract::object_get_by_name(),
        );

        let symbol_site = ctx.ic_site_counter;
        ctx.ic_site_counter += 1;
        let symbol_cache_name = super::super::inline_cache_global_name(ctx, symbol_site);
        ctx.ic_globals.push(symbol_cache_name.clone());

        let field_site = ctx.ic_site_counter;
        ctx.ic_site_counter += 1;
        let field_cache_name = super::super::inline_cache_global_name(ctx, field_site);
        ctx.ic_globals.push(field_cache_name.clone());

        let probe_idx = ctx.new_block("symfield.probe");
        let identity_idx = ctx.new_block("symfield.identity");
        let slot_idx = ctx.new_block("symfield.slot");
        let slot_label = ctx.block_label(slot_idx);
        let hit_idx = ctx.new_block("symfield.hit");
        let live_idx = ctx.new_block("symfield.live");
        let miss_idx = ctx.new_block("symfield.miss");
        let merge_idx = ctx.new_block("symfield.merge");
        let probe_label = ctx.block_label(probe_idx);
        let identity_label = ctx.block_label(identity_idx);
        let hit_label = ctx.block_label(hit_idx);
        let live_label = ctx.block_label(live_idx);
        let miss_label = ctx.block_label(miss_idx);
        let merge_label = ctx.block_label(merge_idx);

        // #9708: both caches sit behind pointer slots the miss handler fills
        // on the first prime. The probe, identity and hit blocks read through
        // the two pointers, so they are reached only once both are present;
        // an absent cache is the miss edge a zero epoch always took.
        let symbol_slot = crate::expr::emit_inline_cache_slot(ctx, &symbol_cache_name);
        let field_slot = crate::expr::emit_inline_cache_slot(ctx, &field_cache_name);
        let symbol_cache = symbol_slot.cache.clone();
        let field_cache = field_slot.cache.clone();
        let both_present = ctx
            .block()
            .and(I1, &symbol_slot.present, &field_slot.present);
        ctx.block()
            .cond_br(&both_present, &probe_label, &miss_label);

        ctx.current_block = probe_idx;
        let epoch = ctx
            .block()
            .load_atomic_acquire(I64, "@PERRY_SYMBOL_PROPERTY_IC_EPOCH", 8);
        let cached_epoch_ptr = ctx.block().gep(I64, &symbol_cache, &[(I64, "0")]);
        let cached_epoch = ctx.block().load_atomic_acquire(I64, &cached_epoch_ptr, 8);
        let epoch_matches = ctx.block().icmp_eq(I64, &epoch, &cached_epoch);
        let base_bits = ctx.block().bitcast_double_to_i64(&base_box);
        let cached_base_ptr = ctx.block().gep(I64, &symbol_cache, &[(I64, "1")]);
        let cached_base = ctx.block().load(I64, &cached_base_ptr);
        let base_matches = ctx.block().icmp_eq(I64, &base_bits, &cached_base);
        let symbol_bits = ctx.block().bitcast_double_to_i64(&symbol_box);
        let cached_symbol_ptr = ctx.block().gep(I64, &symbol_cache, &[(I64, "2")]);
        let cached_symbol = ctx.block().load(I64, &cached_symbol_ptr);
        let symbol_matches = ctx.block().icmp_eq(I64, &symbol_bits, &cached_symbol);
        let identity_matches = ctx.block().and(I1, &base_matches, &symbol_matches);
        let identity_matches = ctx.block().and(I1, &epoch_matches, &identity_matches);
        ctx.block()
            .cond_br(&identity_matches, &identity_label, &miss_label);

        // The epoch/identity edge is what makes dereferencing cache[3] safe:
        // any collection that could relocate this weak value changes the epoch
        // first. Named-property mutations do not, so independently validate
        // the intermediate object's live ShapeId and descriptor latch.
        ctx.current_block = identity_idx;
        let base_raw = ctx.block().and(I64, &base_bits, POINTER_MASK_I64);
        let base_shape_addr = ctx.block().add(I64, &base_raw, "4");
        let base_shape_ptr = ctx.block().inttoptr(I64, &base_shape_addr);
        let base_shape = ctx.block().load(I32, &base_shape_ptr);
        let base_shape = ctx.block().zext(I32, &base_shape, I64);
        let entry_ptr = ctx.block().gep(I64, &symbol_cache, &[(I64, "3")]);
        let entry = ctx.block().load(I64, &entry_ptr);
        let cached_shape = ctx.block().lshr(I64, &entry, "32");
        let shape_matches = ctx.block().icmp_eq(I64, &base_shape, &cached_shape);
        ctx.block()
            .cond_br(&shape_matches, &slot_label, &miss_label);
        ctx.current_block = slot_idx;
        let slot = ctx.block().and(I64, &entry, "4294967295");
        let offset = ctx.block().shl(I64, &slot, "3");
        let fields = ctx.block().add(I64, &base_raw, "16");
        let addr = ctx.block().add(I64, &fields, &offset);
        let intermediate_ptr = ctx.block().inttoptr(I64, &addr);
        let intermediate_bits = ctx.block().load(I64, &intermediate_ptr);
        let intermediate_handle = ctx.block().and(I64, &intermediate_bits, POINTER_MASK_I64);
        let descriptor_addr = ctx.block().sub(I64, &intermediate_handle, "6");
        let descriptor_ptr = ctx.block().inttoptr(I64, &descriptor_addr);
        let gc_flags = ctx.block().load(I16, &descriptor_ptr);
        let descriptor_bits = ctx.block().and(I16, &gc_flags, "2048");
        let data_only = ctx.block().icmp_eq(I16, &descriptor_bits, "0");
        let shape_addr = ctx.block().add(I64, &intermediate_handle, "4");
        let shape_ptr = ctx.block().inttoptr(I64, &shape_addr);
        let shape_id = ctx.block().load(I32, &shape_ptr);
        let shape_token = ctx.block().zext(I32, &shape_id, I64);
        let shape_token = ctx.block().or(I64, &shape_token, "4611686018427387904");
        let cached_token_ptr = ctx.block().gep(I64, &field_cache, &[(I64, "0")]);
        let cached_token = ctx.block().load(I64, &cached_token_ptr);
        let shape_matches = ctx.block().icmp_eq(I64, &shape_token, &cached_token);
        let hit = ctx.block().and(I1, &data_only, &shape_matches);
        ctx.block().cond_br(&hit, &hit_label, &miss_label);

        ctx.current_block = hit_idx;
        let cached_slot_ptr = ctx.block().gep(I64, &field_cache, &[(I64, "1")]);
        let cached_slot = ctx.block().load(I64, &cached_slot_ptr);
        let slot_bytes = ctx.block().shl(I64, &cached_slot, "3");
        let fields_base = ctx.block().add(I64, &intermediate_handle, "16");
        let field_addr = ctx.block().add(I64, &fields_base, &slot_bytes);
        let field_ptr = ctx.block().inttoptr(I64, &field_addr);
        let hit_value = ctx.block().load(DOUBLE, &field_ptr);
        let hit_bits = ctx.block().bitcast_double_to_i64(&hit_value);
        let deleted = ctx
            .block()
            .icmp_eq(I64, &hit_bits, crate::nanbox::TAG_HOLE_I64);
        ctx.block().cond_br(&deleted, &miss_label, &live_label);

        ctx.current_block = live_idx;
        crate::expr::emit_typed_feedback_record_call(
            ctx.block(),
            "js_typed_feedback_record_guard_pass",
            &[(I64, &feedback_site_id)],
        );
        let hit_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = miss_idx;
        let key_index = ctx.strings.intern(property);
        let key_global = format!("@{}", ctx.strings.entry(key_index).handle_global);
        let key_box = ctx.block().load(DOUBLE, &key_global);
        let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
        let key_handle = ctx.block().and(I64, &key_bits, POINTER_MASK_I64);
        let key_ptr = ctx.block().inttoptr(I64, &key_handle);
        let miss_value = ctx.block().call(
            DOUBLE,
            "js_object_get_symbol_then_field_ic_miss",
            &[
                (DOUBLE, &base_box),
                (DOUBLE, &symbol_box),
                (PTR, &key_ptr),
                (I64, &feedback_site_id),
                (PTR, &symbol_slot.slot_ref),
                (PTR, &field_slot.slot_ref),
            ],
        );
        let miss_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = merge_idx;
        Ok(ctx
            .block()
            .phi(DOUBLE, &[(&hit_value, &hit_end), (&miss_value, &miss_end)]))
    })
}

/// Emit the object-backed Array-subclass tier for a guarded `.length` miss.
///
/// Cache words are scalar facts only: exact `(class, ShapeId)` or the stable
/// named-prefix token, the `length` slot, and the live inline-slot bound.  A
/// hit reloads every object/meta/spill pointer from the current receiver, so a
/// moving collection never needs to visit the per-site global.
pub(super) fn emit_array_subclass_length_ic(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    recv_bits: &str,
    recv_handle: &str,
    outer_merge_label: &str,
) -> (String, String) {
    // An ordinary length site has already tested the typed-array tier inline.
    // Its cold tier stays inline too, so the proven case never crosses a call
    // boundary or re-tests that tier.
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache_name = super::super::inline_cache_global_name(ctx, site_id);
    ctx.ic_globals.push(cache_name.clone());
    let slot = format!("@{cache_name}");
    emit_array_subclass_length_body(
        ctx,
        recv_box,
        recv_bits,
        recv_handle,
        &slot,
        outer_merge_label,
    )
}

/// A record's compact hot guard proves only an ordinary array. Every other
/// receiver, including typed arrays, takes the one shared cold body.
fn emit_record_length_cold(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    outer_merge_label: &str,
) -> (String, String) {
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache_name = super::super::inline_cache_global_name(ctx, site_id);
    ctx.ic_globals.push(cache_name.clone());

    let helper_name = format!("perry_length_cold_{}", ctx.module_slug);
    if !ctx.pending_helpers.iter().any(|f| f.name == helper_name) {
        let mut helper = crate::function::LlFunction::new(
            &helper_name,
            DOUBLE,
            vec![(DOUBLE, "%receiver".into()), (PTR, "%cache_slot".into())],
        );
        helper.linkage = "internal".into();
        helper.no_inline = true;
        let saved_func = std::mem::replace(ctx.func, helper);
        let saved_block = ctx.current_block;
        ctx.current_block = ctx.new_block("entry");
        let bits = ctx.block().bitcast_double_to_i64("%receiver");
        let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
        let done = ctx.new_block("return");
        let done_label = ctx.block_label(done);
        let value = emit_nonintrinsic_length_body(
            ctx,
            "%receiver",
            &bits,
            &handle,
            "%cache_slot",
            &done_label,
        );
        ctx.current_block = done;
        ctx.block().ret(DOUBLE, &value);
        let helper = std::mem::replace(ctx.func, saved_func);
        ctx.current_block = saved_block;
        ctx.pending_helpers.push(helper);
    }
    let slot = format!("@{cache_name}");
    let length = ctx
        .block()
        .call(DOUBLE, &helper_name, &[(DOUBLE, recv_box), (PTR, &slot)]);
    let end = ctx.block().label.clone();
    ctx.block().br(outer_merge_label);
    (length, end)
}

fn emit_nonintrinsic_length_body(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    recv_bits: &str,
    recv_handle: &str,
    slot_ref: &str,
    done_label: &str,
) -> String {
    let header_idx = ctx.new_block("plen.typed_array");
    let slow_idx = ctx.new_block("plen.typed_slow");
    let fast_idx = ctx.new_block("plen.typed_fast");
    let header_label = ctx.block_label(header_idx);
    let slow_label = ctx.block_label(slow_idx);
    let fast_label = ctx.block_label(fast_idx);
    let tag = ctx.block().lshr(I64, recv_bits, "48");
    let tag = ctx.block().and(I64, &tag, "65533");
    let valid_tag = ctx.block().icmp_eq(I64, &tag, "32765");
    let above_band = ctx.block().icmp_ugt(I64, recv_handle, "1048575");
    let valid = ctx.block().and(I1, &valid_tag, &above_band);
    ctx.block().cond_br(&valid, &header_label, &slow_label);
    ctx.current_block = header_idx;
    let header = ctx.block().sub(I64, recv_handle, "8");
    let header = ctx.block().inttoptr(I64, &header);
    let gc_type = ctx.block().load(I8, &header);
    let flags = ctx.block().sub(I64, recv_handle, "7");
    let flags = ctx.block().inttoptr(I64, &flags);
    let flags = ctx.block().load(I8, &flags);
    let forwarded = ctx.block().and(I8, &flags, "128");
    let not_forwarded = ctx.block().icmp_eq(I8, &forwarded, "0");
    // Byte views and typed arrays keep a live length at payload +0,
    // including shared storage: detach/resize update that word. Admit
    // only these proven type bytes, not native-arena views. Own named
    // metadata or prototype edits withdraw the accessor proof before
    // publication; the cold edge uses its pooled literal key.

    emit_typed_length_guard(
        ctx,
        recv_handle,
        &gc_type,
        &not_forwarded,
        &fast_label,
        &slow_label,
    );
    ctx.current_block = fast_idx;
    let ptr = ctx.block().inttoptr(I64, recv_handle);
    let length = ctx.block().load(I32, &ptr);
    let fast = ctx.block().uitofp(I32, &length, DOUBLE);
    let fast_end = ctx.block().label.clone();
    ctx.block().br(done_label);
    ctx.current_block = slow_idx;
    let (slow, slow_end) = emit_array_subclass_length_body(
        ctx,
        recv_box,
        recv_bits,
        recv_handle,
        slot_ref,
        done_label,
    );
    // Both tiers join in the caller-provided helper return block.
    let return_idx = ctx
        .func
        .blocks()
        .iter()
        .position(|b| b.label == done_label)
        .unwrap();
    ctx.current_block = return_idx;
    ctx.block()
        .phi(DOUBLE, &[(&fast, &fast_end), (&slow, &slow_end)])
}

pub(super) fn emit_typed_length_guard(
    ctx: &mut FnCtx<'_>,
    recv_handle: &str,
    gc_type: &str,
    not_forwarded: &str,
    fast_label: &str,
    slow_label: &str,
) {
    let owning_byte = ctx.block().icmp_uge(I8, &gc_type, "64");
    let indexed_byte = ctx.block().icmp_ule(I8, &gc_type, "76");
    let is_typed_array = ctx.block().and(I1, &owning_byte, &indexed_byte);
    let byte_header_idx = ctx.new_block("plen.byte_header");
    let byte_header_label = ctx.block_label(byte_header_idx);
    let view_check_idx = ctx.new_block("plen.view_check");
    let view_check_label = ctx.block_label(view_check_idx);
    ctx.block()
        .cond_br(&is_typed_array, &byte_header_label, &view_check_label);

    // An indexed view keeps its fixed length at payload +0. It is the
    // live length while the view is not length-tracking and its link
    // is a plain owner (no bag, so no own `length`) that is neither
    // RESIZABLE nor DETACHED: only those change an owner's extent.
    ctx.current_block = view_check_idx;
    let view_lo = crate::runtime_abi::BYTES_TYPE_BASE | crate::runtime_abi::BYTES_TYPE_VIEW;
    let view_hi = view_lo | 12;
    let is_view_lo = ctx.block().icmp_uge(I8, &gc_type, &view_lo.to_string());
    let is_view_hi = ctx.block().icmp_ule(I8, &gc_type, &view_hi.to_string());
    let is_view = ctx.block().and(I1, &is_view_lo, &is_view_hi);
    let view_header_idx = ctx.new_block("plen.view_header");
    let view_header_label = ctx.block_label(view_header_idx);
    ctx.block()
        .cond_br(&is_view, &view_header_label, &slow_label);
    ctx.current_block = view_header_idx;
    let h = crate::expr::byte_cell::header_word(ctx.block(), &recv_handle);
    let tracking = ctx.block().and(I64, &h, &(1u64 << 23).to_string());
    let fixed = ctx.block().icmp_eq(I64, &tracking, "0");
    let link_addr = ctx.block().add(
        I64,
        &recv_handle,
        &crate::runtime_abi::BYTES_LINK.to_string(),
    );
    let link_ptr = ctx.block().inttoptr(I64, &link_addr);
    let link = ctx.block().load(PTR, &link_ptr);
    let link = ctx.block().ptrtoint(&link, I64);
    let ho = crate::expr::byte_cell::header_word(ctx.block(), &link);
    let mask = 0xe0u64 | (1 << 24) | (1 << 30);
    let state = ctx.block().and(I64, &ho, &mask.to_string());
    let plain_owner = ctx.block().icmp_eq(
        I64,
        &state,
        &crate::runtime_abi::BYTES_TYPE_BASE.to_string(),
    );
    let view_ok = ctx.block().and(I1, &fixed, &plain_owner);
    let view_ok = ctx.block().and(I1, &view_ok, &not_forwarded);
    let named_invalidated =
        ctx.block()
            .load_atomic_acquire(I8, "@PERRY_TYPED_NAMED_PROPS_INVALIDATED", 1);
    let named_pristine = ctx.block().icmp_eq(I8, &named_invalidated, "0");
    let view_ok = ctx.block().and(I1, &view_ok, &named_pristine);
    ctx.block().cond_br(&view_ok, &fast_label, &slow_label);

    ctx.current_block = byte_header_idx;
    let link_addr = ctx.block().add(
        I64,
        &recv_handle,
        &crate::runtime_abi::BYTES_LINK.to_string(),
    );
    let link_ptr = ctx.block().inttoptr(I64, &link_addr);
    let link = ctx.block().load(PTR, &link_ptr);
    let empty = ctx.block().icmp_eq(PTR, &link, "null");
    let ta_header_ok = ctx.block().and(I1, &empty, &not_forwarded);
    let named_invalidated =
        ctx.block()
            .load_atomic_acquire(I8, "@PERRY_TYPED_NAMED_PROPS_INVALIDATED", 1);
    let named_pristine = ctx.block().icmp_eq(I8, &named_invalidated, "0");
    let ta_ok = ctx.block().and(I1, &ta_header_ok, &named_pristine);
    ctx.block().cond_br(&ta_ok, &fast_label, &slow_label);
}

fn emit_array_subclass_length_body(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    recv_bits: &str,
    recv_handle: &str,
    slot_ref: &str,
    outer_merge_label: &str,
) -> (String, String) {
    let header_idx = ctx.new_block("plen.ic.header");
    let shape_idx = ctx.new_block("plen.ic.shape");
    let shape_probe_idx = ctx.new_block("plen.ic.shape.probe");
    let identity_idx = ctx.new_block("plen.ic.identity");
    let exact_idx = ctx.new_block("plen.ic.exact");
    let family_meta_idx = ctx.new_block("plen.ic.family_meta");
    let family_token_idx = ctx.new_block("plen.ic.family_token");
    let slot_idx = ctx.new_block("plen.ic.slot");
    let inline_idx = ctx.new_block("plen.ic.inline");
    let spill_meta_idx = ctx.new_block("plen.ic.spill_meta");
    let spill_ptr_idx = ctx.new_block("plen.ic.spill_ptr");
    let spill_load_idx = ctx.new_block("plen.ic.spill_load");
    let miss_idx = ctx.new_block("plen.ic.miss");
    let merge_idx = ctx.new_block("plen.ic.merge");
    let header_label = ctx.block_label(header_idx);
    let shape_label = ctx.block_label(shape_idx);
    let shape_probe_label = ctx.block_label(shape_probe_idx);
    let identity_label = ctx.block_label(identity_idx);
    let exact_label = ctx.block_label(exact_idx);
    let family_meta_label = ctx.block_label(family_meta_idx);
    let family_token_label = ctx.block_label(family_token_idx);
    let slot_label = ctx.block_label(slot_idx);
    let inline_label = ctx.block_label(inline_idx);
    let spill_meta_label = ctx.block_label(spill_meta_idx);
    let spill_ptr_label = ctx.block_label(spill_ptr_idx);
    let spill_load_label = ctx.block_label(spill_load_idx);
    let miss_label = ctx.block_label(miss_idx);
    let merge_label = ctx.block_label(merge_idx);

    // This block is reachable for every failed ordinary Array/String guard,
    // including primitives and native handle ids. Validate the exact pointer
    // tag and target heap window before reading a managed header.
    let recv_top16 = ctx.block().lshr(I64, recv_bits, "48");
    let pointer_tag = ctx
        .block()
        .icmp_eq(I64, &recv_top16, crate::nanbox::POINTER_TAG_TOP16_I64);
    let heap_floor =
        crate::target_layout::heap_addr_lower_bound_inclusive(ctx.target_triple).to_string();
    let heap_ceiling =
        crate::target_layout::heap_addr_upper_bound_exclusive(ctx.target_triple).to_string();
    let above_floor = ctx.block().icmp_uge(I64, recv_handle, &heap_floor);
    let below_ceiling = ctx.block().icmp_ult(I64, recv_handle, &heap_ceiling);
    let in_heap = ctx.block().and(I1, &pointer_tag, &above_floor);
    let in_heap = ctx.block().and(I1, &in_heap, &below_ceiling);
    // #9708: the cache sits behind a pointer slot the runtime fills on the
    // first shape-carried prime. It is loaded here, off the receiver's
    // dependency chain, and tested at `plen.ic.shape` — NOT folded into the
    // header guard, because the elements-backed arm between them serves
    // `length` without ever publishing a cache, and a site whose receivers
    // are all elements-backed must keep that arm with a slot that stays null.
    let cache = ctx.block().load(PTR, slot_ref);
    let present = ctx.block().icmp_ne(PTR, &cache, "null");
    let ic_slot = crate::expr::InlineCacheSlot {
        slot_ref: slot_ref.into(),
        cache,
        present,
    };
    let cache_ref = ic_slot.cache.clone();
    ctx.block().cond_br(&in_heap, &header_label, &miss_label);

    ctx.current_block = header_idx;
    let gc_type_addr = ctx.block().sub(I64, recv_handle, "8");
    let gc_type_ptr = ctx.block().inttoptr(I64, &gc_type_addr);
    let gc_type = ctx.block().load(I8, &gc_type_ptr);
    let is_object = ctx.block().icmp_eq(I8, &gc_type, "2");
    let gc_flags_addr = ctx.block().sub(I64, recv_handle, "7");
    let gc_flags_ptr = ctx.block().inttoptr(I64, &gc_flags_addr);
    let gc_flags = ctx.block().load(I8, &gc_flags_ptr);
    let forwarded = ctx.block().and(I8, &gc_flags, "128");
    let not_forwarded = ctx.block().icmp_eq(I8, &forwarded, "0");
    let header_ok = ctx.block().and(I1, &is_object, &not_forwarded);
    let meta_ptr_size: u64 = if crate::target_layout::target_is_ilp32(ctx.target_triple) {
        4
    } else {
        8
    };
    let meta_offset =
        crate::target_layout::object_meta_slot_offset_bytes(ctx.target_triple).to_string();
    // An elements-backed Array-subclass instance (`ObjectMeta.elements`):
    // `length` is the inner Array's length word — no shape IC. A probe miss
    // (no meta, no store) is the shape-carried form and keeps the IC below.
    let elem_meta_idx = ctx.new_block("plen.elem.meta");
    let elem_store_idx = ctx.new_block("plen.elem.store");
    let elem_length_idx = ctx.new_block("plen.elem.length");
    let elem_meta_label = ctx.block_label(elem_meta_idx);
    let elem_store_label = ctx.block_label(elem_store_idx);
    let elem_length_label = ctx.block_label(elem_length_idx);
    ctx.block()
        .cond_br(&header_ok, &elem_meta_label, &miss_label);
    ctx.current_block = elem_meta_idx;
    let elem_meta_addr = ctx.block().add(I64, recv_handle, &meta_offset);
    let elem_meta_slot_ptr = ctx.block().inttoptr(I64, &elem_meta_addr);
    let elem_meta_loaded = ctx.block().load(
        if meta_ptr_size == 4 { I32 } else { I64 },
        &elem_meta_slot_ptr,
    );
    let elem_meta_i64 = if meta_ptr_size == 4 {
        ctx.block().zext(I32, &elem_meta_loaded, I64)
    } else {
        elem_meta_loaded
    };
    let elem_has_meta = ctx.block().icmp_ne(I64, &elem_meta_i64, "0");
    ctx.block()
        .cond_br(&elem_has_meta, &elem_store_label, &shape_label);
    ctx.current_block = elem_store_idx;
    let elem_meta_ptr = ctx.block().inttoptr(I64, &elem_meta_i64);
    // `ObjectMeta.elements` is word 12 (offset 96; const-asserted in the runtime).
    let elem_store_slot_ptr = ctx.block().gep(I64, &elem_meta_ptr, &[(I64, "12")]);
    let elem_store_i64 = ctx.block().load(I64, &elem_store_slot_ptr);
    let elem_has_store = ctx.block().icmp_ne(I64, &elem_store_i64, "0");
    ctx.block()
        .cond_br(&elem_has_store, &elem_length_label, &shape_label);
    ctx.current_block = elem_length_idx;
    let elem_store_ptr = ctx.block().inttoptr(I64, &elem_store_i64);
    let elem_length_i32 = ctx.block().load(I32, &elem_store_ptr);
    let elem_length = ctx.block().uitofp(I32, &elem_length_i32, DOUBLE);
    let elem_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = shape_idx;
    ctx.block()
        .cond_br(&ic_slot.present, &shape_probe_label, &miss_label);

    ctx.current_block = shape_probe_idx;
    let object_ptr = ctx.block().inttoptr(I64, recv_handle);
    let class_id = ctx.block().load(I32, &object_ptr);
    let shape_addr = ctx.block().add(I64, recv_handle, "4");
    let shape_ptr = ctx.block().inttoptr(I64, &shape_addr);
    let shape_id = ctx.block().load(I32, &shape_ptr);
    let class64 = ctx.block().zext(I32, &class_id, I64);
    let shape64 = ctx.block().zext(I32, &shape_id, I64);
    let class_high = ctx.block().shl(I64, &class64, "32");
    let live_key = ctx.block().or(I64, &class_high, &shape64);
    let cached_key_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "0")]);
    let cached_key = ctx.block().load(I64, &cached_key_ptr);
    let key_nonzero = ctx.block().icmp_ne(I64, &cached_key, "0");
    ctx.block()
        .cond_br(&key_nonzero, &identity_label, &miss_label);

    ctx.current_block = identity_idx;
    let family_token_bit = crate::nanbox::i64_literal(1u64 << 63);
    let family_bits = ctx.block().and(I64, &cached_key, &family_token_bit);
    let is_family = ctx.block().icmp_ne(I64, &family_bits, "0");
    ctx.block()
        .cond_br(&is_family, &family_meta_label, &exact_label);

    ctx.current_block = exact_idx;
    let exact_match = ctx.block().icmp_eq(I64, &live_key, &cached_key);
    ctx.block().cond_br(&exact_match, &slot_label, &miss_label);

    ctx.current_block = family_meta_idx;
    let family_meta_addr = ctx.block().add(I64, recv_handle, &meta_offset);
    let family_meta_slot_ptr = ctx.block().inttoptr(I64, &family_meta_addr);
    let family_meta_loaded = ctx.block().load(
        if meta_ptr_size == 4 { I32 } else { I64 },
        &family_meta_slot_ptr,
    );
    let family_meta_i64 = if meta_ptr_size == 4 {
        ctx.block().zext(I32, &family_meta_loaded, I64)
    } else {
        family_meta_loaded
    };
    let family_has_meta = ctx.block().icmp_ne(I64, &family_meta_i64, "0");
    ctx.block()
        .cond_br(&family_has_meta, &family_token_label, &miss_label);

    ctx.current_block = family_token_idx;
    let family_meta_ptr = ctx.block().inttoptr(I64, &family_meta_i64);
    let family_token_ptr = ctx.block().gep(I64, &family_meta_ptr, &[(I64, "6")]);
    let live_family_token = ctx.block().load(I64, &family_token_ptr);
    let family_match = ctx.block().icmp_eq(I64, &live_family_token, &cached_key);
    ctx.block().cond_br(&family_match, &slot_label, &miss_label);

    ctx.current_block = slot_idx;
    let length_slot_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "1")]);
    let length_slot = ctx.block().load(I64, &length_slot_ptr);
    let inline_bound_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "2")]);
    let inline_bound = ctx.block().load(I64, &inline_bound_ptr);
    let length_is_inline = ctx.block().icmp_ult(I64, &length_slot, &inline_bound);
    ctx.block()
        .cond_br(&length_is_inline, &inline_label, &spill_meta_label);

    ctx.current_block = inline_idx;
    let object_header_size =
        crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let length_bytes = ctx.block().shl(I64, &length_slot, "3");
    let length_offset = ctx.block().add(I64, &length_bytes, &object_header_size);
    let length_addr = ctx.block().add(I64, recv_handle, &length_offset);
    let length_ptr = ctx.block().inttoptr(I64, &length_addr);
    let inline_length = ctx.block().load(DOUBLE, &length_ptr);
    let inline_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = spill_meta_idx;
    let spill_meta_addr = ctx.block().add(I64, recv_handle, &meta_offset);
    let spill_meta_slot_ptr = ctx.block().inttoptr(I64, &spill_meta_addr);
    let spill_meta_loaded = ctx.block().load(
        if meta_ptr_size == 4 { I32 } else { I64 },
        &spill_meta_slot_ptr,
    );
    let spill_meta_i64 = if meta_ptr_size == 4 {
        ctx.block().zext(I32, &spill_meta_loaded, I64)
    } else {
        spill_meta_loaded
    };
    let has_meta = ctx.block().icmp_ne(I64, &spill_meta_i64, "0");
    ctx.block()
        .cond_br(&has_meta, &spill_ptr_label, &miss_label);

    ctx.current_block = spill_ptr_idx;
    let spill_meta_ptr = ctx.block().inttoptr(I64, &spill_meta_i64);
    let spill_slot_ptr = ctx.block().gep(I64, &spill_meta_ptr, &[(I64, "4")]);
    let spill_i64 = ctx.block().load(I64, &spill_slot_ptr);
    let has_spill = ctx.block().icmp_ne(I64, &spill_i64, "0");
    let safe_spill_i64 = ctx
        .block()
        .select(I1, &has_spill, I64, &spill_i64, &spill_meta_i64);
    let spill_ptr = ctx.block().inttoptr(I64, &safe_spill_i64);
    let spill_len = ctx.block().load(I32, &spill_ptr);
    let spill_len_i64 = ctx.block().zext(I32, &spill_len, I64);
    let length_in_spill = ctx.block().icmp_ult(I64, &length_slot, &spill_len_i64);
    let spill_ok = ctx.block().and(I1, &has_spill, &length_in_spill);
    ctx.block()
        .cond_br(&spill_ok, &spill_load_label, &miss_label);

    ctx.current_block = spill_load_idx;
    let spill_element_word = ctx.block().add(I64, &length_slot, "1");
    let spill_element_ptr =
        ctx.block()
            .gep_inbounds(I64, &spill_ptr, &[(I64, &spill_element_word)]);
    let spilled_length = ctx.block().load(DOUBLE, &spill_element_ptr);
    let spill_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = miss_idx;
    let key_idx = ctx.strings.intern("length");
    let key_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let key_box = ctx.block().load(DOUBLE, &key_global);
    let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
    let key_raw = ctx.block().and(I64, &key_bits, POINTER_MASK_I64);
    let miss_length = ctx.block().call(
        DOUBLE,
        "js_value_length_property_key_ic_f64",
        &[
            (DOUBLE, recv_box),
            (PTR, &ic_slot.slot_ref),
            (I64, &key_raw),
        ],
    );
    let miss_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    let length = ctx.block().phi(
        DOUBLE,
        &[
            (&elem_length, &elem_end),
            (&inline_length, &inline_end),
            (&spilled_length, &spill_end),
            (&miss_length, &miss_end),
        ],
    );
    let end = ctx.block().label.clone();
    ctx.block().br(outer_merge_label);
    (length, end)
}

/// The record's captured array proof narrows only its hot guard. All cold
/// property semantics still have the same per-module backend.
pub(super) fn emit_record_length(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    recv_bits: &str,
    recv_handle: &str,
) -> String {
    let tagged = ctx
        .block()
        .and(I64, recv_bits, &crate::nanbox::i64_literal(!(1u64 << 49)));
    let base = crate::nanbox::i64_literal((0x7ffdu64 << 48) | 1048576);
    let offset = ctx.block().sub(I64, &tagged, &base);
    let valid = ctx.block().icmp_ult(I64, &offset, "281474975662080");
    let check = ctx.new_block("plen.check_gc");
    let fast = ctx.new_block("plen.fast");
    let slow = ctx.new_block("plen.slow");
    let merge = ctx.new_block("plen.merge");
    let cl = ctx.block_label(check);
    let fl = ctx.block_label(fast);
    let sl = ctx.block_label(slow);
    let ml = ctx.block_label(merge);
    ctx.block().cond_br(&valid, &cl, &sl);
    ctx.current_block = check;
    let header = ctx.block().sub(I64, recv_handle, "8");
    let header = ctx.block().inttoptr(I64, &header);
    let word = ctx.block().load(crate::types::I16, &header);
    let kind = ctx.block().and(crate::types::I16, &word, "33021");
    let live = ctx.block().icmp_eq(crate::types::I16, &kind, "1");
    ctx.block().cond_br(&live, &fl, &sl);
    ctx.current_block = fast;
    let ptr = ctx.block().inttoptr(I64, recv_handle);
    let length = ctx.block().load(I32, &ptr);
    let length = ctx.block().uitofp(I32, &length, DOUBLE);
    let fp = ctx.block().label.clone();
    ctx.block().br(&ml);
    ctx.current_block = slow;
    let (cold, sp) = emit_record_length_cold(ctx, recv_box, &ml);
    ctx.current_block = merge;
    ctx.block().phi(DOUBLE, &[(&length, &fp), (&cold, &sp)])
}
