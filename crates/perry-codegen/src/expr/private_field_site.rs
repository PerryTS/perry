//! #11791: a compiled instance private field access, `recv.#x` and
//! `recv.#x = v`, whose hit is one shape compare and the field's slot.
//!
//! A private field is an `ENTRY_PRIVATE` entry of its holder's key list
//! (#11791): its presence, its slot, the live inline bound and the slot's
//! representation lane are all facts of the receiver's ShapeId. The site owns
//! one word (`emit_private_site_cache`) that the runtime publishes on a miss
//! (`js_private_field_site_get` / `_set` -> `private_field_site_word`):
//!
//! ```text
//!   ShapeId (bits 0..32) | slot (bits 32..48) | F64 lane (bit 48)
//! ```
//!
//! and only when the field is a plain inline slot whose lane is `Any` or
//! `F64`. A hit is therefore the same proof the public class-field fast path
//! has (its ShapeId names the slot and the lane), so the read is one load and
//! the write is a raw double store into an `F64` lane (a non-finite or
//! non-Number value takes the miss, whose checked store generalizes the
//! lane), or a barriered store into an `Any` lane. The private brand check
//! needs nothing more: the entry being in the shape IS the field being
//! initialized, and a fresh evaluation of the class template clears every
//! hit through the template bit (`emit_private_template_inert`).
//!
//! The miss is one runtime call that runs the brand check (which throws, or
//! publishes the word) and then the ordinary by-name access of the field's
//! storage key, which also resolves a fresh evaluation's storage.
//!
//! Only `this` and plain local receivers take this path: the receiver is
//! evaluated once, before a write's right-hand side, and the brand check runs
//! after it, as `PrivateSet` does.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

use super::{
    emit_private_site_cache, emit_private_template_inert, emit_string_literal_global, lower_expr,
    FnCtx,
};

/// The word's `F64` lane bit (runtime `PRIVATE_FIELD_SITE_F64`, `1 << 48`).
const PRIVATE_FIELD_SITE_F64: &str = "281474976710656";
/// The word's slot bits after `>> 32` (runtime `PRIVATE_FIELD_SITE_SLOT_MASK`).
const PRIVATE_FIELD_SITE_SLOT_MASK: &str = "65535";
/// `POINTER_TAG + 0x10_0000`: with the unsigned compare below, "a POINTER
/// NaN-box above the native-handle band" in one test (as
/// `emit_private_site_guard`).
const POINTER_BAND_BIAS: &str = "9222527611925692416";
const POINTER_BAND_SPAN: &str = "281474975662080";

/// The parts of an instance private FIELD access this module compiles.
pub(crate) struct PrivateFieldSite<'e> {
    receiver: &'e Expr,
    class_id: u32,
    field_name: &'e str,
    receiver_is_brand_owner: bool,
    static_final: Option<StaticFinal>,
}

/// The declaring class's completed static shape (#11791,
/// `codegen::static_private_class`): its id, the field's slot in it and
/// whether that slot is an `F64` lane. A receiver carrying the id has the
/// field there by the id's facts, with no site word to load.
#[derive(Clone, Copy)]
struct StaticFinal {
    id: u32,
    slot: u32,
    f64: bool,
}

/// `object` (the object of a `PropertyGet`/`PropertySet` of the private
/// storage key `property`) as an instance private field site of operation
/// `op` (0 read, 1 write), when its receiver is `this` or a local.
pub(crate) fn private_field_site<'e>(
    ctx: &FnCtx<'_>,
    object: &'e Expr,
    property: &str,
    op: u8,
) -> Option<PrivateFieldSite<'e>> {
    let Expr::PrivateGuard {
        class_name,
        class_id,
        field_name,
        kind,
        op: guard_op,
        receiver_is_brand_owner,
        object: receiver,
    } = object
    else {
        return None;
    };
    if *kind != 0 || *guard_op != op {
        return None;
    }
    // A scalar-replaced constructor has no heap `this`.
    if !ctx.scalar_ctor_target.is_empty() {
        return None;
    }
    if !matches!(receiver.as_ref(), Expr::This | Expr::LocalGet(_)) {
        return None;
    }
    let class_id = if *class_id != 0 {
        *class_id
    } else {
        ctx.class_ids.get(class_name).copied().unwrap_or(0)
    };
    if class_id == 0 {
        return None;
    }
    let static_final =
        crate::codegen::static_private_class::static_private_field_slot(ctx, class_name, property)
            .map(|(id, slot, f64)| StaticFinal { id, slot, f64 });
    Some(PrivateFieldSite {
        receiver,
        class_id,
        field_name,
        receiver_is_brand_owner: *receiver_is_brand_owner,
        static_final,
    })
}

/// Where a hit's slot comes from: the completed static shape, or the site
/// word the runtime published.
enum SiteHit {
    Static(StaticFinal),
    Word(String),
}

struct Probe {
    payload: String,
    hits: Vec<(usize, SiteHit)>,
    miss_idx: usize,
}

/// The inline half shared by reads and writes: the receiver tests, then the
/// shape compared with the declaring class's completed static id (when there
/// is one) and with the site word. Returns each hit block with where its slot
/// comes from; the miss block is empty. Leaves no block current.
fn emit_site_probe(
    ctx: &mut FnCtx<'_>,
    obj: &str,
    class_id: u32,
    site: &str,
    static_final: Option<StaticFinal>,
) -> Probe {
    let shape_idx = ctx.new_block("pfield.shape");
    let miss_idx = ctx.new_block("pfield.miss");
    let shape_l = ctx.block_label(shape_idx);
    let miss_l = ctx.block_label(miss_idx);
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(obj);
    let biased = blk.sub(I64, &bits, POINTER_BAND_BIAS);
    let in_range = blk.icmp_ult(I64, &biased, POINTER_BAND_SPAN);
    let inert = emit_private_template_inert(blk, class_id);
    let ready = blk.and(I1, &in_range, &inert);
    blk.cond_br(&ready, &shape_l, &miss_l);

    ctx.current_block = shape_idx;
    let blk = ctx.block();
    let payload = blk.add(I64, &biased, "1048576");
    let shape_addr = blk.add(I64, &payload, "4");
    let shape_ptr = blk.inttoptr(I64, &shape_addr);
    let shape = blk.load(I32, &shape_ptr);
    let mut hits = Vec::new();
    if let Some(sf) = static_final {
        let hit_idx = ctx.new_block("pfield.hit_final");
        let word_idx = ctx.new_block("pfield.word");
        let hit_l = ctx.block_label(hit_idx);
        let word_l = ctx.block_label(word_idx);
        let blk = ctx.block();
        let is_final = blk.icmp_eq(I32, &shape, &sf.id.to_string());
        blk.cond_br(&is_final, &hit_l, &word_l);
        hits.push((hit_idx, SiteHit::Static(sf)));
        ctx.current_block = word_idx;
    }
    let hit_idx = ctx.new_block("pfield.hit");
    let hit_l = ctx.block_label(hit_idx);
    let blk = ctx.block();
    let word = blk.load_atomic_monotonic(I64, site, 8);
    let primed = blk.icmp_ne(I64, &word, "0");
    let word_shape = blk.trunc(I64, &word, I32);
    let same = blk.icmp_eq(I32, &shape, &word_shape);
    let hit = blk.and(I1, &primed, &same);
    blk.cond_br(&hit, &hit_l, &miss_l);
    hits.push((hit_idx, SiteHit::Word(word)));
    Probe {
        payload,
        hits,
        miss_idx,
    }
}

/// The slot address a hit block reads or writes: the payload's inline
/// fields plus `slot * 8`.
fn emit_slot_ptr(ctx: &mut FnCtx<'_>, hit: &SiteHit, payload: &str) -> String {
    let header_skip = crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let blk = ctx.block();
    let slot = match hit {
        SiteHit::Static(sf) => sf.slot.to_string(),
        SiteHit::Word(word) => {
            let slot = blk.lshr(I64, word, "32");
            blk.and(I64, &slot, PRIVATE_FIELD_SITE_SLOT_MASK)
        }
    };
    let obj_ptr = blk.inttoptr(I64, payload);
    let fields = blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
    blk.gep(DOUBLE, &fields, &[(I64, &slot)])
}

/// The brand owner operand of the runtime miss, as the guard arm passes it.
fn lower_brand_owner(
    ctx: &mut FnCtx<'_>,
    site: &PrivateFieldSite<'_>,
    obj: &str,
) -> Result<String> {
    if site.receiver_is_brand_owner {
        Ok(obj.to_string())
    } else {
        Ok(super::this_super_call::load_private_brand_owner(ctx))
    }
}

fn storage_key_box(ctx: &mut FnCtx<'_>, property: &str) -> String {
    let key_idx = ctx.strings.intern(property);
    let key_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    ctx.block().load(DOUBLE, &key_global)
}

/// `recv.#x`.
pub(crate) fn lower_get(
    ctx: &mut FnCtx<'_>,
    site: PrivateFieldSite<'_>,
    property: &str,
) -> Result<String> {
    let obj = lower_expr(ctx, site.receiver)?;
    let brand_owner = lower_brand_owner(ctx, &site, &obj)?;
    let name_label = emit_string_literal_global(ctx, site.field_name);
    let cache = emit_private_site_cache(ctx, 1);
    let probe = emit_site_probe(ctx, &obj, site.class_id, &cache, site.static_final);
    let join_idx = ctx.new_block("pfield.join");
    let join_l = ctx.block_label(join_idx);

    let mut incoming: Vec<(String, String)> = Vec::new();
    for (hit_idx, hit) in &probe.hits {
        ctx.current_block = *hit_idx;
        let slot_ptr = emit_slot_ptr(ctx, hit, &probe.payload);
        let value = ctx.block().load(DOUBLE, &slot_ptr);
        incoming.push((value, ctx.block_label(ctx.current_block)));
        ctx.block().br(&join_l);
    }

    ctx.current_block = probe.miss_idx;
    let key = storage_key_box(ctx, property);
    let miss_val = ctx.block().call(
        DOUBLE,
        "js_private_field_site_get",
        &[
            (DOUBLE, &obj),
            (DOUBLE, &brand_owner),
            (I32, &site.class_id.to_string()),
            (PTR, &name_label),
            (I32, &site.field_name.len().to_string()),
            (PTR, &cache),
            (DOUBLE, &key),
        ],
    );
    incoming.push((miss_val, ctx.block_label(ctx.current_block)));
    ctx.block().br(&join_l);

    ctx.current_block = join_idx;
    let incoming: Vec<(&str, &str)> = incoming
        .iter()
        .map(|(v, l)| (v.as_str(), l.as_str()))
        .collect();
    Ok(ctx.block().phi(DOUBLE, &incoming))
}

/// `recv.#x = value`. Returns the assigned value.
pub(crate) fn lower_set(
    ctx: &mut FnCtx<'_>,
    site: PrivateFieldSite<'_>,
    property: &str,
    value: &Expr,
) -> Result<String> {
    // The receiver is live across the value's lowering, which is arbitrary
    // user code: root both, as the class-field store does for these
    // receivers (`with_class_store_operands`).
    let operands: [&Expr; 2] = [site.receiver, value];
    let (vals, group) = crate::lower_call::lower_operand_list_rooted(ctx, &operands)?;
    let obj = vals[0].clone();
    let val = vals[1].clone();
    let result = lower_set_operands(ctx, &site, property, value, &obj, &val);
    group.release(ctx);
    result
}

/// Store into an `F64` lane: a plain finite double only; anything else is the
/// miss, whose checked store keeps the lane or generalizes it. Returns the
/// storing block's label.
fn emit_raw_store(
    ctx: &mut FnCtx<'_>,
    val: &str,
    slot_ptr: &str,
    miss_l: &str,
    join_l: &str,
) -> String {
    let store_idx = ctx.new_block("pfield.set.raw_store");
    let store_l = ctx.block_label(store_idx);
    {
        let blk = ctx.block();
        let val_bits = blk.bitcast_double_to_i64(val);
        let finite =
            super::class_field_inline_guard::emit_plain_finite_number_check(blk, &val_bits);
        blk.cond_br(&finite, &store_l, miss_l);
    }
    ctx.current_block = store_idx;
    let blk = ctx.block();
    // GC_STORE_AUDIT(POINTER_FREE): the plain-finite test proved a genuine
    // unboxed double, never a NaN-boxed heap pointer.
    blk.store(DOUBLE, val, slot_ptr);
    blk.br(join_l);
    store_l
}

/// Store into an `Any` lane: the ordinary barriered JSValue slot store.
/// Returns the label of the block that branches to the join.
#[allow(clippy::too_many_arguments)]
fn emit_boxed_store(
    ctx: &mut FnCtx<'_>,
    obj: &str,
    val: &str,
    slot_ptr: &str,
    addref_needed: bool,
    barrier_needed: bool,
    join_l: &str,
) -> String {
    let (obj_bits, obj_handle, slot_addr) = {
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(obj);
        let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
        let slot_addr = blk.ptrtoint(slot_ptr, I64);
        (obj_bits, obj_handle, slot_addr)
    };
    super::emit_jsvalue_slot_store_pointer_tested(
        ctx,
        slot_ptr,
        val,
        &obj_handle,
        addref_needed,
        &obj_bits,
        &slot_addr,
        barrier_needed,
        "private_field_set",
    );
    let end = ctx.block_label(ctx.current_block);
    ctx.block().br(join_l);
    end
}

fn lower_set_operands(
    ctx: &mut FnCtx<'_>,
    site: &PrivateFieldSite<'_>,
    property: &str,
    value: &Expr,
    obj: &str,
    val: &str,
) -> Result<String> {
    let brand_owner = lower_brand_owner(ctx, site, obj)?;
    let name_label = emit_string_literal_global(ctx, site.field_name);
    let cache = emit_private_site_cache(ctx, 1);
    let barrier_needed = !super::expr_produces_non_pointer_bits_by_construction(ctx, value);
    let addref_needed = super::class_field_store_needs_string_addref(ctx, value);
    let probe = emit_site_probe(ctx, obj, site.class_id, &cache, site.static_final);
    let miss_l = ctx.block_label(probe.miss_idx);
    let join_idx = ctx.new_block("pfield.join");
    let join_l = ctx.block_label(join_idx);

    let mut stored: Vec<String> = Vec::new();
    for (hit_idx, hit) in &probe.hits {
        ctx.current_block = *hit_idx;
        let slot_ptr = emit_slot_ptr(ctx, hit, &probe.payload);
        match hit {
            SiteHit::Static(sf) if sf.f64 => {
                stored.push(emit_raw_store(ctx, val, &slot_ptr, &miss_l, &join_l));
            }
            SiteHit::Static(_) => {
                stored.push(emit_boxed_store(
                    ctx,
                    obj,
                    val,
                    &slot_ptr,
                    addref_needed,
                    barrier_needed,
                    &join_l,
                ));
            }
            SiteHit::Word(word) => {
                let raw_idx = ctx.new_block("pfield.set.raw");
                let boxed_idx = ctx.new_block("pfield.set.boxed");
                let raw_l = ctx.block_label(raw_idx);
                let boxed_l = ctx.block_label(boxed_idx);
                {
                    let blk = ctx.block();
                    let lane = blk.and(I64, word, PRIVATE_FIELD_SITE_F64);
                    let is_f64 = blk.icmp_ne(I64, &lane, "0");
                    blk.cond_br(&is_f64, &raw_l, &boxed_l);
                }
                ctx.current_block = raw_idx;
                stored.push(emit_raw_store(ctx, val, &slot_ptr, &miss_l, &join_l));
                ctx.current_block = boxed_idx;
                stored.push(emit_boxed_store(
                    ctx,
                    obj,
                    val,
                    &slot_ptr,
                    addref_needed,
                    barrier_needed,
                    &join_l,
                ));
            }
        }
    }

    ctx.current_block = probe.miss_idx;
    let key = storage_key_box(ctx, property);
    let miss_val = ctx.block().call(
        DOUBLE,
        "js_private_field_site_set",
        &[
            (DOUBLE, obj),
            (DOUBLE, &brand_owner),
            (I32, &site.class_id.to_string()),
            (PTR, &name_label),
            (I32, &site.field_name.len().to_string()),
            (PTR, &cache),
            (DOUBLE, &key),
            (DOUBLE, val),
        ],
    );
    let miss_end = ctx.block_label(ctx.current_block);
    ctx.block().br(&join_l);

    ctx.current_block = join_idx;
    let mut incoming: Vec<(&str, &str)> = stored.iter().map(|l| (val, l.as_str())).collect();
    incoming.push((&miss_val, &miss_end));
    Ok(ctx.block().phi(DOUBLE, &incoming))
}
