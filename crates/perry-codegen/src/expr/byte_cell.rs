//! One header-word admission and owner/view resolution for byte cells.
use super::FnCtx;
use crate::types::{DOUBLE, I1, I32, I64, PTR};

pub(crate) fn materialize_param(ctx: &mut FnCtx<'_>, id: u32, boxed: &str, brands: &[u8]) {
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
    ctx.block().emit_raw(format!(
        "; bytes.hoist.roots receiver={} owner={}",
        receiver_root_slot.trim_start_matches('%'),
        owner_root_slot.trim_start_matches('%')
    ));
    ctx.block().store(DOUBLE, boxed, &receiver_root_slot);
    let access = crate::collectors::ByteViewParamAccess {
        valid_i1: "false".into(),
        data_i64: "0".into(),
        receiver_root_slot,
        owner_root_slot,
        data_slot: ctx.func.alloca_entry(I64),
        length_slot: ctx.func.alloca_entry(I32),
        valid_slot: ctx.func.alloca_entry(I1),
        brands: brands.to_vec(),
    };
    refresh_param(ctx, &access);
    ctx.receiver_descriptors
        .materialize_byte_view_param(id, access);
}

pub(crate) fn keep_cached_owner_alive(
    ctx: &mut FnCtx<'_>,
    access: &crate::collectors::ByteViewParamAccess,
) {
    for slot in [&access.receiver_root_slot, &access.owner_root_slot] {
        let value = ctx.block().load(DOUBLE, slot);
        let bits = ctx.block().bitcast_double_to_i64(&value);
        ctx.block().emit_raw(format!(
            "call void asm sideeffect \"\", \"r\"(i64 {bits}) \"gc-leaf-function\""
        ));
    }
}

fn refresh_param(ctx: &mut FnCtx<'_>, access: &crate::collectors::ByteViewParamAccess) {
    let miss = ctx.new_block("bytes.hoist.miss");
    let done = ctx.new_block("bytes.hoist.done");
    let miss_l = ctx.block_label(miss);
    let done_l = ctx.block_label(done);
    let receiver = ctx.block().load(DOUBLE, &access.receiver_root_slot);
    let resolved = resolve(ctx, &receiver, &access.brands, &miss_l);
    let bits = ctx
        .block()
        .or(I64, &resolved.owner, crate::nanbox::POINTER_TAG_I64);
    let owner = ctx.block().bitcast_i64_to_double(&bits);
    let hit_l = ctx.block().label.clone();
    ctx.block().br(&done_l);
    ctx.current_block = miss;
    ctx.block().br(&done_l);
    ctx.current_block = done;
    let data = ctx
        .block()
        .phi(I64, &[(&resolved.data, &hit_l), ("0", &miss_l)]);
    let len = ctx
        .block()
        .phi(I32, &[(&resolved.len, &hit_l), ("0", &miss_l)]);
    let valid = ctx.block().phi(I1, &[("true", &hit_l), ("false", &miss_l)]);
    let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let owner = ctx
        .block()
        .phi(DOUBLE, &[(&owner, &hit_l), (&undef, &miss_l)]);
    ctx.block().store(DOUBLE, &owner, &access.owner_root_slot);
    ctx.block().store(I64, &data, &access.data_slot);
    ctx.block().store(I32, &len, &access.length_slot);
    ctx.block().store(I1, &valid, &access.valid_slot);
}

pub(crate) fn refresh_hoisted_byte_accesses(ctx: &mut FnCtx<'_>) {
    for access in ctx.receiver_descriptors.hoisted_byte_params() {
        keep_cached_owner_alive(ctx, &access);
        refresh_param(ctx, &access);
    }
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
    if let Some(param) = super::u8_buffer_read::byte_view_param_for(ctx, object) {
        let admitted = ctx.new_block("bytes.hoisted.read");
        let admitted_l = ctx.block_label(admitted);
        ctx.block().cond_br(&param.valid_i1, &admitted_l, miss);
        ctx.current_block = admitted;
        let len = ctx.block().load(I32, &param.length_slot);
        let bits = ctx.block().bitcast_double_to_i64(boxed);
        let raw = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
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
    let inline = ctx.new_block("bytes.inline");
    let external = ctx.new_block("bytes.external");
    let done = ctx.new_block("bytes.ready");
    let labels = [
        header, owner, view, view_owner, store, inline, external, done,
    ]
    .map(|b| ctx.block_label(b));
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
    let o = ctx.block().load(PTR, &link_ptr);
    let o = ctx.block().ptrtoint(&o, I64);
    let ho = header_word(ctx.block(), &o);
    let mask = 0xe0u64 | (1 << 23) | (1 << 24) | (1 << 30);
    let state = ctx.block().and(I64, &ho, &mask.to_string());
    let inl = ctx.block().icmp_eq(
        I64,
        &state,
        &crate::runtime_abi::BYTES_TYPE_BASE.to_string(),
    );
    let ool = ctx.block().icmp_eq(
        I64,
        &state,
        &(crate::runtime_abi::BYTES_TYPE_BASE as u64 | (1 << 23)).to_string(),
    );
    let admitted = ctx.block().or(I1, &inl, &ool);
    // Shared and NativeArena owners retain their atomic/disposal runtime rules.
    let owner_brand = ctx.block().and(I64, &ho, "31");
    let shared = ctx.block().icmp_eq(I64, &owner_brand, "15");
    let arena = ctx.block().icmp_eq(I64, &owner_brand, "18");
    let special = ctx.block().or(I1, &shared, &arena);
    let regular = ctx.block().icmp_eq(I1, &special, "false");
    let admitted = ctx.block().and(I1, &admitted, &regular);
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
    let base = ctx
        .block()
        .add(I64, &owning, &crate::runtime_abi::BYTES_STORE.to_string());
    let flag = ctx.block().and(I64, &word, &(1u64 << 23).to_string());
    let is_ool = ctx.block().icmp_ne(I64, &flag, "0");
    ctx.block().cond_br(&is_ool, &labels[6], &labels[5]);
    ctx.current_block = inline;
    let inline_end = ctx.block().label.clone();
    ctx.block().br(&labels[7]);
    ctx.current_block = external;
    let slot = ctx.block().inttoptr(I64, &base);
    let data = ctx.block().load(PTR, &slot);
    let data = ctx.block().ptrtoint(&data, I64);
    let external_end = ctx.block().label.clone();
    ctx.block().br(&labels[7]);
    ctx.current_block = done;
    let base = ctx
        .block()
        .phi(I64, &[(&base, &inline_end), (&data, &external_end)]);
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

/// Stores additionally reject a frozen receiver before deriving a writable access.
pub(crate) fn resolve_write(ctx: &mut FnCtx<'_>, boxed: &str, brands: &[u8], miss: &str) -> Access {
    let access = resolve(ctx, boxed, brands, miss);
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

pub(crate) fn inline_owner_guard(ctx: &mut FnCtx<'_>, boxed: &str, brand: u8) -> (String, String) {
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
    let ty = ctx
        .block()
        .and(I64, &h, &(0xffu64 | (1 << 16) | (1 << 23)).to_string());
    let guard = ctx.block().icmp_eq(I64, &ty, &brand.to_string());
    ctx.block().br(&done_l);
    ctx.current_block = done;
    let guard = ctx
        .block()
        .phi(I1, &[("false", &before), (&guard, &inspect_l)]);
    (raw, guard)
}
