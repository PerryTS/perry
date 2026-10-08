//! One header-word admission and owner/view resolution for byte cells.
use super::FnCtx;
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

/// Install the hoisted access proof of `id` for exactly `brands`. Every slot
/// is initialized in the entry block, and the proof starts dirty. Installation
/// must precede body lowering so every call edge can invalidate the proof.
pub(crate) fn materialize_param(ctx: &mut FnCtx<'_>, id: u32, _boxed: &str, brands: &[u8]) {
    let mut access = install_loop_access(ctx, id, brands);
    // Callers install only parameters that are never reassigned or mutated.
    access.fixed_receiver = true;
    ctx.receiver_descriptors
        .materialize_byte_view_param(id, access);
}

/// Look up a proof installed by the pre-pass. An unregistered access must
/// resolve its header each time: earlier calls cannot dirty a later install.
pub(crate) fn access_for(
    ctx: &mut FnCtx<'_>,
    id: u32,
    brands: &[u8],
) -> Option<crate::collectors::ByteViewParamAccess> {
    #[cfg(test)]
    if std::env::var("PERRY_B4_SABOTAGE").ok().as_deref() == Some("lazy_install") {
        return Some(install_loop_access(ctx, id, brands));
    }
    ctx.receiver_descriptors
        .byte_view_access(id, brands)
        .cloned()
}

/// Install only during parameter/loop preparation, before lowering calls.
pub(super) fn install_loop_access(
    ctx: &mut FnCtx<'_>,
    id: u32,
    brands: &[u8],
) -> crate::collectors::ByteViewParamAccess {
    if let Some(access) = ctx.receiver_descriptors.byte_view_access(id, brands) {
        return access.clone();
    }
    let receiver_root_slot = ctx.func.alloca_entry(DOUBLE);
    let owner_root_slot = ctx.func.alloca_entry(DOUBLE);
    let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    for slot in [&receiver_root_slot, &owner_root_slot] {
        ctx.func.entry_allocas_push_store(DOUBLE, &undef, slot);
    }
    super::scalar_slot_root::root_entry_alloca(ctx, &receiver_root_slot);
    #[cfg(test)]
    let omit_owner = std::env::var("PERRY_B4_SABOTAGE").ok().as_deref() == Some("hoist_owner");
    #[cfg(not(test))]
    let omit_owner = false;
    if !omit_owner {
        super::scalar_slot_root::root_entry_alloca(ctx, &owner_root_slot);
    }
    // A hoisted data address points into the owner's store, never into a
    // view cell, so only the owner must stay live across each call edge.
    ctx.block().retain_byte_owner_root_slot(&owner_root_slot);
    let state_slot = ctx.func.alloca_entry(I8);
    ctx.block().emit_raw(format!(
        "; bytes.hoist.roots receiver={} owner={} state={}",
        receiver_root_slot.trim_start_matches('%'),
        owner_root_slot.trim_start_matches('%'),
        state_slot.trim_start_matches('%')
    ));
    let access = crate::collectors::ByteViewParamAccess {
        valid_i1: "false".into(),
        data_i64: "0".into(),
        receiver_root_slot,
        owner_root_slot,
        owner_raw_slot: ctx.func.alloca_entry(I64),
        data_slot: ctx.func.alloca_entry(I64),
        length_slot: ctx.func.alloca_entry(I32),
        valid_slot: state_slot,
        bits_slot: ctx.func.alloca_entry(I64),
        fixed_receiver: false,
        brands: brands.to_vec(),
    };
    ctx.func
        .entry_allocas_push_store(I64, "0", &access.owner_raw_slot);
    ctx.func
        .entry_allocas_push_store(I64, "0", &access.data_slot);
    ctx.func
        .entry_allocas_push_store(I32, "0", &access.length_slot);
    ctx.func
        .entry_allocas_push_store(I8, "0", &access.valid_slot);
    ctx.func
        .entry_allocas_push_store(I64, "0", &access.bits_slot);
    #[cfg(test)]
    let skip_dirty = std::env::var("PERRY_B4_SABOTAGE").ok().as_deref() == Some("call_dirty");
    #[cfg(not(test))]
    let skip_dirty = false;
    if !skip_dirty {
        ctx.func
            .reg_counter()
            .push_byte_access_dirty_slot(access.valid_slot.clone());
    }
    ctx.receiver_descriptors
        .materialize_byte_view_param(id, access.clone());
    access
}

/// Resolve `boxed` into the proof's slots and mark it clean. Only a receiver
/// that admits one of the proof's brands, is not frozen, and has a plain
/// owner (see [`resolve`]) is valid; everything else takes the runtime arm.
fn refresh_param(
    ctx: &mut FnCtx<'_>,
    access: &crate::collectors::ByteViewParamAccess,
    boxed: &str,
) {
    let miss = ctx.new_block("bytes.hoist.miss");
    let done = ctx.new_block("bytes.hoist.done");
    let miss_l = ctx.block_label(miss);
    let done_l = ctx.block_label(done);
    ctx.block().store(DOUBLE, boxed, &access.receiver_root_slot);
    let bits = ctx.block().bitcast_double_to_i64(boxed);
    ctx.block().store(I64, &bits, &access.bits_slot);
    let resolved = resolve(ctx, boxed, &access.brands, &miss_l);
    // Writes need no per-store frozen test while the proof is clean: freezing
    // is a call, and a call dirties the proof.
    let own = header_word(ctx.block(), &resolved.raw);
    let frozen = ctx.block().and(I64, &own, &(1u64 << 16).to_string());
    let writable = ctx.block().icmp_eq(I64, &frozen, "0");
    let owner_bits = ctx
        .block()
        .or(I64, &resolved.owner, crate::nanbox::POINTER_TAG_I64);
    let owner = ctx.block().bitcast_i64_to_double(&owner_bits);
    let hit_l = ctx.block().label.clone();
    ctx.block().br(&done_l);
    ctx.current_block = miss;
    ctx.block().br(&done_l);
    ctx.current_block = done;
    let data = ctx
        .block()
        .phi(I64, &[(&resolved.data, &hit_l), ("0", &miss_l)]);
    let owner_raw = ctx
        .block()
        .phi(I64, &[(&resolved.owner, &hit_l), ("0", &miss_l)]);
    let len = ctx
        .block()
        .phi(I32, &[(&resolved.len, &hit_l), ("0", &miss_l)]);
    let valid = ctx
        .block()
        .phi(I1, &[(&writable, &hit_l), ("false", &miss_l)]);
    let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let owner = ctx
        .block()
        .phi(DOUBLE, &[(&owner, &hit_l), (&undef, &miss_l)]);
    ctx.block().store(DOUBLE, &owner, &access.owner_root_slot);
    ctx.block().store(I64, &owner_raw, &access.owner_raw_slot);
    ctx.block().store(I64, &data, &access.data_slot);
    ctx.block().store(I32, &len, &access.length_slot);
    let state = ctx.block().select(I1, &valid, I8, "1", "2");
    ctx.block().store(I8, &state, &access.valid_slot);
}

/// Revalidate the proof unless it is clean for exactly this receiver.
/// Leaves the current block where the proof's slots are current.
pub(crate) fn revalidate(
    ctx: &mut FnCtx<'_>,
    access: &crate::collectors::ByteViewParamAccess,
    boxed: &str,
) {
    let reval = ctx.new_block("bytes.access.revalidate");
    let ready = ctx.new_block("bytes.access.ready");
    let reval_l = ctx.block_label(reval);
    let ready_l = ctx.block_label(ready);
    let check = ctx.new_block("bytes.access.check");
    let check_l = ctx.block_label(check);
    let state = ctx.block().load(I8, &access.valid_slot);
    let valid = ctx.block().icmp_eq(I8, &state, "1");
    let same = if access.fixed_receiver {
        "true".to_string()
    } else {
        let bits = ctx.block().bitcast_double_to_i64(boxed);
        let proven = ctx.block().load(I64, &access.bits_slot);
        ctx.block().icmp_eq(I64, &bits, &proven)
    };
    let hit = ctx.block().and(I1, &valid, &same);
    ctx.block().cond_br(&hit, &ready_l, &check_l);
    // A clean proof of a receiver that is not admitted keeps its runtime arm
    // without re-resolving; a dirty or rebound proof resolves again.
    ctx.current_block = check;
    let rejected = ctx.block().icmp_eq(I8, &state, "2");
    let settled = ctx.block().and(I1, &rejected, &same);
    ctx.block().cond_br(&settled, &ready_l, &reval_l);
    ctx.current_block = reval;
    refresh_param(ctx, access, boxed);
    ctx.block().br(&ready_l);
    ctx.current_block = ready;
}

pub(crate) fn brand_for_kind(kind: u8) -> u8 {
    crate::runtime_abi::BYTES_TYPE_BASE | [2, 0, 4, 3, 6, 5, 8, 9, 1, 10, 11, 7][kind as usize]
}

pub(crate) struct Access {
    pub raw: String,
    pub word: String,
    pub owner: String,
    pub data: String,
    pub len: String,
}

pub(crate) fn resolve_read(
    ctx: &mut FnCtx<'_>,
    object: &perry_hir::Expr,
    boxed: &str,
    brands: &[u8],
    miss: &str,
) -> Access {
    if let Some(param) = super::u8_buffer_read::byte_view_param_for(ctx, object, boxed, brands) {
        // The handle dominates both arms: runtime misses consume it too.
        let bits = ctx.block().bitcast_double_to_i64(boxed);
        let raw = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
        let admitted = ctx.new_block("bytes.hoisted.read");
        let admitted_l = ctx.block_label(admitted);
        ctx.block().cond_br(&param.valid_i1, &admitted_l, miss);
        ctx.current_block = admitted;
        let len = ctx.block().load(I32, &param.length_slot);
        Access {
            raw: raw.clone(),
            word: String::new(),
            owner: raw,
            data: param.data_i64,
            len,
        }
    } else {
        resolve(ctx, boxed, brands, miss)
    }
}

pub(crate) fn header_word(blk: &mut crate::block::LlBlock, raw: &str) -> String {
    let addr = blk.sub(I64, raw, &crate::runtime_abi::GC_HEADER_SIZE.to_string());
    let ptr = blk.inttoptr(I64, &addr);
    blk.load(I64, &ptr)
}

/// The caller uses the result only on this function's passing continuation.
/// All failures branch to its existing runtime arm. No call or safepoint occurs.
pub(crate) fn resolve(ctx: &mut FnCtx<'_>, boxed: &str, brands: &[u8], miss: &str) -> Access {
    let header = ctx.new_block("bytes.header");
    let owner = ctx.new_block("bytes.owner");
    let view = ctx.new_block("bytes.view");
    let view_owner = ctx.new_block("bytes.view.owner");
    let store = ctx.new_block("bytes.store");
    let labels = [header, owner, view, view_owner, store].map(|b| ctx.block_label(b));
    let bits = ctx.block().bitcast_double_to_i64(boxed);
    let raw = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let tag = ctx.block().and(
        I64,
        &bits,
        &crate::nanbox::i64_literal(crate::nanbox::TAG_MASK),
    );
    let ptr = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::POINTER_TAG_I64);
    let floor =
        crate::target_layout::heap_addr_lower_bound_inclusive(ctx.target_triple).to_string();
    let above = ctx.block().icmp_uge(I64, &raw, &floor);
    let ceiling =
        crate::target_layout::heap_addr_upper_bound_exclusive(ctx.target_triple).to_string();
    let below = ctx.block().icmp_ult(I64, &raw, &ceiling);
    let low = ctx.block().and(I64, &raw, "7");
    let aligned = ctx.block().icmp_eq(I64, &low, "0");
    let valid = ctx.block().and(I1, &ptr, &above);
    let valid = ctx.block().and(I1, &valid, &below);
    let valid = ctx.block().and(I1, &valid, &aligned);
    ctx.block().cond_br(&valid, &labels[0], miss);
    ctx.current_block = header;
    let h = header_word(ctx.block(), &raw);
    let t = ctx.block().and(I64, &h, &(0xffu64 | (1 << 23)).to_string());
    let mut owning = "false".to_string();
    let mut viewing = "false".to_string();
    for brand in brands {
        let role = ctx.block().and(I64, &t, "255");
        let o = ctx.block().icmp_eq(I64, &role, &brand.to_string());
        owning = ctx.block().or(I1, &owning, &o);
        let v = ctx.block().icmp_eq(
            I64,
            &t,
            &(brand | crate::runtime_abi::BYTES_TYPE_VIEW).to_string(),
        );
        viewing = ctx.block().or(I1, &viewing, &v);
    }
    ctx.block().cond_br(&owning, &labels[1], &labels[2]);
    ctx.current_block = owner;
    let owner_end = ctx.block().label.clone();
    ctx.block().br(&labels[4]);
    ctx.current_block = view;
    ctx.block().cond_br(&viewing, &labels[3], miss);
    ctx.current_block = view_owner;
    let link = ctx
        .block()
        .add(I64, &raw, &crate::runtime_abi::BYTES_LINK.to_string());
    let link_ptr = ctx.block().inttoptr(I64, &link);
    let linked = ctx.block().load(PTR, &link_ptr);
    let linked = ctx.block().ptrtoint(&linked, I64);
    let linked_word = header_word(ctx.block(), &linked);
    // A view links its owner or its bag, and a bag holds the owner, boxed, at
    // its fixed first slot `BYTES_VIEW_BAG_OWNER`. The hop is straight-line:
    // a bag reads that slot, a direct link re-reads the view's own link word
    // (always readable, and its untagged pointer survives the mask). No block
    // or join is added, so the resolution keeps main's CFG and the owner arm
    // is untouched by views.
    let linked_type = ctx.block().and(I64, &linked_word, "255");
    let bagged = ctx.block().icmp_eq(
        I64,
        &linked_type,
        &crate::runtime_abi::GC_TYPE_OBJECT.to_string(),
    );
    let bag_slot = ctx.block().add(
        I64,
        &linked,
        &crate::runtime_abi::BYTES_VIEW_BAG_OWNER.to_string(),
    );
    let owner_addr = ctx.block().select(I1, &bagged, I64, &bag_slot, &link);
    let owner_ptr = ctx.block().inttoptr(I64, &owner_addr);
    let owner_bits = ctx.block().load(I64, &owner_ptr);
    let o = ctx
        .block()
        .and(I64, &owner_bits, crate::nanbox::POINTER_MASK_I64);
    let ho = header_word(ctx.block(), &o);
    let admitted = plain_owner(ctx.block(), &ho);
    let offset_addr = ctx
        .block()
        .add(I64, &raw, &crate::runtime_abi::BYTES_AUX.to_string());
    let offset_ptr = ctx.block().inttoptr(I64, &offset_addr);
    let offset = ctx.block().load(I32, &offset_ptr);
    let offset = ctx.block().zext(I32, &offset, I64);
    let view_end = ctx.block().label.clone();
    ctx.block().cond_br(&admitted, &labels[4], miss);
    ctx.current_block = store;
    let owning = ctx.block().phi(I64, &[(&raw, &owner_end), (&o, &view_end)]);
    let word = ctx.block().phi(I64, &[(&h, &owner_end), (&ho, &view_end)]);
    let offset = ctx
        .block()
        .phi(I64, &[("0", &owner_end), (&offset, &view_end)]);
    let base = owner_data(ctx, &owning, &word, "bytes");
    let data = ctx.block().add(I64, &base, &offset);
    let len_ptr = ctx.block().inttoptr(I64, &raw);
    let len = ctx.block().load(I32, &len_ptr);
    Access {
        raw,
        word: h,
        owner: owning,
        data,
        len,
    }
}

/// Resolve an admitted owner's store. The header decides whether BYTES_STORE
/// is the first inline byte or the out-of-line data word. This does not admit
/// receivers or extend a pointer's lifetime: callers retain their existing
/// owner roots and hoist only under their existing storage proof.
pub(crate) fn owner_data(ctx: &mut FnCtx<'_>, raw: &str, word: &str, prefix: &str) -> String {
    let base = ctx
        .block()
        .add(I64, raw, &crate::runtime_abi::BYTES_STORE.to_string());
    #[cfg(test)]
    if std::env::var("PERRY_B4_SABOTAGE").ok().as_deref() == Some("inline_data") {
        return base;
    }
    let inline = ctx.new_block(&format!("{prefix}.inline"));
    let external = ctx.new_block(&format!("{prefix}.external"));
    let done = ctx.new_block(&format!("{prefix}.ready"));
    let inline_l = ctx.block_label(inline);
    let external_l = ctx.block_label(external);
    let done_l = ctx.block_label(done);
    let flag = ctx.block().and(I64, word, &(1u64 << 23).to_string());
    let is_ool = ctx.block().icmp_ne(I64, &flag, "0");
    ctx.block().cond_br(&is_ool, &external_l, &inline_l);
    ctx.current_block = inline;
    ctx.block().br(&done_l);
    ctx.current_block = external;
    let slot = ctx.block().inttoptr(I64, &base);
    let data = ctx.block().load(PTR, &slot);
    let data = ctx.block().ptrtoint(&data, I64);
    ctx.block().br(&done_l);
    ctx.current_block = done;
    ctx.block()
        .phi(I64, &[(&base, &inline_l), (&data, &external_l)])
}

/// A view's owner header admits the emitted path when it is an owner role,
/// Inline or OutOfLine, neither RESIZABLE nor DETACHED, and not a Shared or
/// NativeArena owner (those keep their atomic/disposal runtime rules). A bag
/// header (`GC_TYPE_OBJECT`) never passes.
fn plain_owner(blk: &mut crate::block::LlBlock, ho: &str) -> String {
    let mask = 0xe0u64 | (1 << 23) | (1 << 24) | (1 << 30);
    let state = blk.and(I64, ho, &mask.to_string());
    let inl = blk.icmp_eq(
        I64,
        &state,
        &crate::runtime_abi::BYTES_TYPE_BASE.to_string(),
    );
    let ool = blk.icmp_eq(
        I64,
        &state,
        &(crate::runtime_abi::BYTES_TYPE_BASE as u64 | (1 << 23)).to_string(),
    );
    let admitted = blk.or(I1, &inl, &ool);
    let owner_brand = blk.and(I64, ho, "31");
    let shared = blk.icmp_eq(I64, &owner_brand, "15");
    let arena = blk.icmp_eq(I64, &owner_brand, "18");
    let special = blk.or(I1, &shared, &arena);
    let regular = blk.icmp_eq(I1, &special, "false");
    #[cfg(test)]
    let shared_admit = std::env::var("PERRY_B4_SABOTAGE").ok().as_deref() == Some("shared_admit");
    #[cfg(not(test))]
    let shared_admit = false;
    blk.and(I1, &admitted, if shared_admit { "true" } else { &regular })
}

/// Stores additionally reject a frozen receiver before deriving a writable access.
pub(crate) fn resolve_write(ctx: &mut FnCtx<'_>, boxed: &str, brands: &[u8], miss: &str) -> Access {
    let access = resolve(ctx, boxed, brands, miss);
    admit_write(ctx, access, miss)
}

pub(crate) fn resolve_indexed_write(
    ctx: &mut FnCtx<'_>,
    object: &perry_hir::Expr,
    boxed: &str,
    brands: &[u8],
    miss: &str,
) -> Access {
    let access = resolve_read(ctx, object, boxed, brands, miss);
    if access.word.is_empty() {
        // A clean proof is valid only for a receiver that was not frozen.
        return access;
    }
    admit_write(ctx, access, miss)
}

fn admit_write(ctx: &mut FnCtx<'_>, access: Access, miss: &str) -> Access {
    let flags = ctx
        .block()
        .and(I64, &access.word, &(1u64 << 16).to_string());
    let writable = ctx.block().icmp_eq(I64, &flags, "0");
    let store = ctx.new_block("bytes.writable");
    let store_l = ctx.block_label(store);
    ctx.block().cond_br(&writable, &store_l, miss);
    ctx.current_block = store;
    access
}

/// Translate the type-byte brand to the runtime kind without a memory lookup.
pub(crate) fn kind_and_width(blk: &mut crate::block::LlBlock, h: &str) -> (String, String) {
    let brand = blk.and(I64, h, "31");
    let nibble = blk.shl(I64, &brand, "2");
    const KINDS: [u8; 12] = [1, 8, 0, 3, 2, 5, 4, 11, 6, 7, 9, 10];
    let packed = KINDS
        .iter()
        .enumerate()
        .fold(0u64, |p, (i, k)| p | ((*k as u64) << (i * 4)));
    let kind = blk.lshr(I64, &packed.to_string(), &nibble);
    let kind = blk.and(I64, &kind, "15");
    let two = blk.shl(I64, &brand, "1");
    let packed = crate::runtime_abi::BYTES_ELEMENT_SHIFT[..12]
        .iter()
        .enumerate()
        .fold(0u64, |p, (i, s)| p | ((*s as u64) << (i * 2)));
    let shift = blk.lshr(I64, &packed.to_string(), &two);
    let shift = blk.and(I64, &shift, "3");
    (kind, blk.shl(I64, "1", &shift))
}

pub(crate) fn owner_guard(ctx: &mut FnCtx<'_>, boxed: &str, brand: u8) -> (String, String) {
    let inspect = ctx.new_block("bytes.inline.guard");
    let done = ctx.new_block("bytes.inline.admission");
    let inspect_l = ctx.block_label(inspect);
    let done_l = ctx.block_label(done);
    let bits = ctx.block().bitcast_double_to_i64(boxed);
    let raw = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let tag = ctx.block().and(
        I64,
        &bits,
        &crate::nanbox::i64_literal(crate::nanbox::TAG_MASK),
    );
    let tagged = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::POINTER_TAG_I64);
    let floor =
        crate::target_layout::heap_addr_lower_bound_inclusive(ctx.target_triple).to_string();
    let above = ctx.block().icmp_uge(I64, &raw, &floor);
    let ceiling =
        crate::target_layout::heap_addr_upper_bound_exclusive(ctx.target_triple).to_string();
    let below = ctx.block().icmp_ult(I64, &raw, &ceiling);
    let low = ctx.block().and(I64, &raw, "7");
    let aligned = ctx.block().icmp_eq(I64, &low, "0");
    let g = ctx.block().and(I1, &tagged, &above);
    let g = ctx.block().and(I1, &g, &below);
    let g = ctx.block().and(I1, &g, &aligned);
    let before = ctx.block().label.clone();
    ctx.block().cond_br(&g, &inspect_l, &done_l);
    ctx.current_block = inspect;
    let h = header_word(ctx.block(), &raw);
    let ty = ctx.block().and(
        I64,
        &h,
        &(0xffu64 | (1 << 16) | (1 << 24) | (1 << 30)).to_string(),
    );
    let guard = ctx.block().icmp_eq(I64, &ty, &brand.to_string());
    ctx.block().br(&done_l);
    ctx.current_block = done;
    let guard = ctx
        .block()
        .phi(I1, &[("false", &before), (&guard, &inspect_l)]);
    (raw, guard)
}

/// A tracked local may be consumed solely through its interior data slot.
/// Retain both exact object starts across every subsequent safepoint. Fresh
/// constructor results have a direct owner link, before any bag attachment.
pub(crate) fn retain_fresh_local_owner(ctx: &mut FnCtx<'_>, boxed: &str) {
    let receiver_slot = ctx.func.alloca_entry(DOUBLE);
    let owner_slot = ctx.func.alloca_entry(DOUBLE);
    let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    for slot in [&receiver_slot, &owner_slot] {
        ctx.func.entry_allocas_push_store(DOUBLE, &undef, slot);
    }
    super::scalar_slot_root::root_entry_alloca(ctx, &receiver_slot);
    #[cfg(test)]
    let omit_owner = std::env::var("PERRY_B4_SABOTAGE").ok().as_deref() == Some("hoist_owner");
    #[cfg(not(test))]
    let omit_owner = false;
    if !omit_owner {
        super::scalar_slot_root::root_entry_alloca(ctx, &owner_slot);
    }
    ctx.block().emit_raw(format!(
        "; bytes.local.roots receiver={} owner={}",
        receiver_slot.trim_start_matches('%'),
        owner_slot.trim_start_matches('%')
    ));
    ctx.block().store(DOUBLE, boxed, &receiver_slot);
    let raw = super::unbox_to_i64(ctx.block(), boxed);
    let h = header_word(ctx.block(), &raw);
    let role = ctx
        .block()
        .and(I64, &h, &crate::runtime_abi::BYTES_TYPE_VIEW.to_string());
    let is_view = ctx.block().icmp_ne(I64, &role, "0");
    let view = ctx.new_block("bytes.local.owner.view");
    let done = ctx.new_block("bytes.local.owner.done");
    let view_l = ctx.block_label(view);
    let done_l = ctx.block_label(done);
    let owner_l = ctx.block().label.clone();
    ctx.block().cond_br(&is_view, &view_l, &done_l);
    ctx.current_block = view;
    let link = ctx
        .block()
        .add(I64, &raw, &crate::runtime_abi::BYTES_LINK.to_string());
    let link_ptr = ctx.block().inttoptr(I64, &link);
    let owner = ctx.block().load(I64, &link_ptr);
    let bits = ctx.block().or(I64, &owner, crate::nanbox::POINTER_TAG_I64);
    let owner = ctx.block().bitcast_i64_to_double(&bits);
    let view_end = ctx.block().label.clone();
    ctx.block().br(&done_l);
    ctx.current_block = done;
    let owner = ctx
        .block()
        .phi(DOUBLE, &[(boxed, &owner_l), (&owner, &view_end)]);
    ctx.block().store(DOUBLE, &owner, &owner_slot);
    // The data slot addresses the owner's store; the owner alone is retained
    // across call edges.
    ctx.block().retain_byte_owner_root_slot(&owner_slot);
    ctx.receiver_descriptors
        .retain_byte_owner(receiver_slot, owner_slot);
}

#[cfg(test)]
#[path = "native_owner_tests.rs"]
mod native_owner_tests;

#[cfg(test)]
mod tests {
    use perry_hir::{types::Type, Expr, Stmt};

    fn add_indexed_loop_read(function: &mut perry_hir::Function) {
        let mut body = std::mem::take(&mut function.body);
        body.insert(
            0,
            Stmt::Expr(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Integer(0)),
            }),
        );
        function.body = vec![Stmt::While {
            condition: Expr::Bool(true),
            body,
        }];
    }

    #[test]
    fn tracked_local_keeps_receiver_and_owner_roots_across_a_collecting_call() {
        crate::temp_root_coverage::under_both_lowerings(|mode| {
            for alias in [false, true] {
                let ir = crate::temp_root_coverage::main_ir_for(
                    "byte_local_owner",
                    vec![
                        Stmt::Let {
                            id: 1,
                            name: "bytes".into(),
                            ty: Type::Named("Uint32Array".into()),
                            mutable: false,
                            init: Some(Expr::TypedArrayNew {
                                kind: perry_hir::TYPED_ARRAY_KIND_UINT32,
                                arg: Some(Box::new(Expr::Integer(64))),
                            }),
                        },
                        Stmt::Let {
                            id: 2,
                            name: "alias".into(),
                            ty: Type::Named("Uint32Array".into()),
                            mutable: false,
                            init: Some(Expr::LocalGet(1)),
                        },
                        Stmt::While {
                            condition: Expr::Integer(1),
                            body: vec![
                                Stmt::Expr(Expr::Call {
                                    callee: Box::new(Expr::GlobalGet(100)),
                                    args: vec![],
                                    type_args: vec![],
                                    byte_offset: 0,
                                }),
                                Stmt::Expr(Expr::IndexGet {
                                    object: Box::new(Expr::LocalGet(if alias { 2 } else { 1 })),
                                    index: Box::new(Expr::Integer(0)),
                                }),
                            ],
                        },
                    ],
                );
                let marker = ir
                    .lines()
                    .find(|line| line.contains("; bytes.local.roots "))
                    .expect("the tracked data slot must retain its exact owners");
                let roots = crate::testing::root_slots::bound_slots(&ir);
                for field in ["receiver=", "owner="] {
                    let slot = marker
                        .split(field)
                        .nth(1)
                        .unwrap()
                        .split_whitespace()
                        .next()
                        .unwrap();
                    let slot = format!("%{slot}");
                    assert!(
                        roots.contains_key(&slot)
                            || ir.contains(&format!("{slot} = alloca ptr addrspace(1)")),
                        "{mode}: {field}{slot} must be a statepoint root"
                    );
                }
                assert!(
                    ir.contains("asm sideeffect"),
                    "the owner must remain live after the call"
                );
            }
        });
    }

    #[test]
    fn buffer_parameter_numeric_read_retains_owner_and_uses_common_header() {
        use perry_hir::{Function, Module, Param};
        crate::temp_root_coverage::under_both_lowerings(|mode| {
            let mut module = Module::new("buffer_param_owner.ts");
            module.functions.push(Function {
                id: 10,
                name: "read".into(),
                type_params: vec![],
                params: vec![Param {
                    id: 1,
                    name: "view".into(),
                    ty: Type::Named("Buffer".into()),
                    default: None,
                    decorators: vec![],
                    is_rest: false,
                    arguments_object: None,
                }],
                return_type: Type::Number,
                body: vec![
                    Stmt::Expr(Expr::Call {
                        callee: Box::new(Expr::GlobalGet(100)),
                        args: vec![],
                        type_args: vec![],
                        byte_offset: 0,
                    }),
                    Stmt::Return(Some(Expr::Call {
                        callee: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(1)),
                            property: "readInt32BE".into(),
                            byte_offset: 0,
                        }),
                        args: vec![Expr::Integer(0)],
                        type_args: vec![],
                        byte_offset: 0,
                    })),
                ],
                is_async: false,
                is_generator: false,
                is_strict: true,
                is_exported: false,
                captures: vec![],
                decorators: vec![],
                was_plain_async: false,
                was_unrolled: false,
            });
            // A loop indexed read registers the proof before any call. A
            // numeric intrinsic alone deliberately keeps per-access resolution.
            add_indexed_loop_read(&mut module.functions[0]);
            let ir = String::from_utf8(
                crate::compile_module(
                    &module,
                    crate::CompileOptions {
                        emit_ir_only: true,
                        ..Default::default()
                    },
                )
                .unwrap(),
            )
            .unwrap();
            assert!(
                ir.contains("bytes.numeric.load"),
                "{mode}: numeric load missing:\n{ir}"
            );
            assert!(
                !ir.contains("call ptr @js_native_buffer_data_ptr"),
                "{mode}: legacy preheader"
            );
            let marker = ir
                .lines()
                .find(|line| line.contains("; bytes.hoist.roots "))
                .unwrap();
            let roots = crate::testing::root_slots::bound_slots(&ir);
            for field in ["receiver=", "owner="] {
                let slot = format!(
                    "%{}",
                    marker
                        .split(field)
                        .nth(1)
                        .unwrap()
                        .split_whitespace()
                        .next()
                        .unwrap()
                );
                assert!(
                    roots.contains_key(&slot)
                        || ir.contains(&format!("{slot} = alloca ptr addrspace(1)")),
                    "{mode}: {field}{slot} must be a statepoint root"
                );
            }
            assert!(
                ir.contains("asm sideeffect"),
                "{mode}: keep owner live after collection"
            );
        });
    }

    #[test]
    fn dropping_buffer_parameter_owner_root_turns_the_invariant_red() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "expr::byte_cell::tests::buffer_parameter_numeric_read_retains_owner_and_uses_common_header", "--nocapture"])
            .env("PERRY_B4_SABOTAGE", "hoist_owner").output().unwrap();
        assert!(String::from_utf8_lossy(&child.stdout).contains("running 1 test"));
        assert!(
            !child.status.success(),
            "missing Buffer owner root must be detected"
        );
    }

    #[test]
    fn dropping_a_tracked_local_owner_root_turns_the_invariant_red() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "expr::byte_cell::tests::tracked_local_keeps_receiver_and_owner_roots_across_a_collecting_call", "--nocapture"])
            .env("PERRY_B4_SABOTAGE", "hoist_owner").output().unwrap();
        assert!(String::from_utf8_lossy(&child.stdout).contains("running 1 test"));
        assert!(
            !child.status.success(),
            "the planted missing owner root must be detected"
        );
    }
    /// Every executed call dirties a hoisted byte-access proof at the call
    /// itself (the LlBlock choke point), so the next access revalidates and
    /// no expression merge pays for a refresh.
    #[test]
    fn executed_calls_dirty_the_hoisted_byte_access_proof() {
        use perry_hir::{Function, Module, Param};
        let mut module = Module::new("byte_proof_dirty.ts");
        module.functions.push(Function {
            id: 10,
            name: "read".into(),
            type_params: vec![],
            params: vec![Param {
                id: 1,
                name: "view".into(),
                ty: Type::Named("Buffer".into()),
                default: None,
                decorators: vec![],
                is_rest: false,
                arguments_object: None,
            }],
            return_type: Type::Number,
            body: vec![
                Stmt::Expr(Expr::Call {
                    callee: Box::new(Expr::GlobalGet(100)),
                    args: vec![],
                    type_args: vec![],
                    byte_offset: 0,
                }),
                Stmt::Return(Some(Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(1)),
                        property: "readInt32BE".into(),
                        byte_offset: 0,
                    }),
                    args: vec![Expr::Integer(0)],
                    type_args: vec![],
                    byte_offset: 0,
                })),
            ],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: vec![],
            decorators: vec![],
            was_plain_async: false,
            was_unrolled: false,
        });
        add_indexed_loop_read(&mut module.functions[0]);
        let ir = String::from_utf8(
            crate::compile_module(
                &module,
                crate::CompileOptions {
                    emit_ir_only: true,
                    ..Default::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
        let states: Vec<String> = ir
            .lines()
            .filter(|line| line.contains("; bytes.hoist.roots "))
            .map(|marker| {
                format!(
                    "store i8 0, ptr %{}",
                    marker
                        .split("state=")
                        .nth(1)
                        .unwrap()
                        .split_whitespace()
                        .next()
                        .unwrap()
                )
            })
            .collect();
        assert!(!states.is_empty(), "the Buffer parameter installs a proof");
        let lines: Vec<&str> = ir.lines().map(str::trim).collect();
        let dirtied_call = lines.iter().enumerate().any(|(index, line)| {
            if !line.contains("call double @js_closure_call0(") {
                return false;
            }
            let dirty_run: Vec<&str> = lines[..index]
                .iter()
                .rev()
                .copied()
                .take_while(|line| line.starts_with("store i8 0, ptr "))
                .collect();
            states
                .iter()
                .all(|state| dirty_run.contains(&state.as_str()))
        });
        assert!(
            dirtied_call,
            "a call must dirty the proof right before it:\n{ir}"
        );
        assert!(
            ir.contains("bytes.access.revalidate"),
            "the access must revalidate a dirty proof:\n{ir}"
        );
    }

    #[test]
    fn a_call_that_leaves_the_proof_clean_turns_the_witness_red() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "expr::byte_cell::tests::executed_calls_dirty_the_hoisted_byte_access_proof",
                "--nocapture",
            ])
            .env("PERRY_B4_SABOTAGE", "call_dirty")
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&child.stdout).contains("running 1 test"));
        assert!(
            !child.status.success(),
            "a proof that survives a call must be detected"
        );
    }
}
