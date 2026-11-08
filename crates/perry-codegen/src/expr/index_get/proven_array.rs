//! Element reads whose receiver wpfacts proves (representation RFC §10.7,
//! phase P4; wpfacts `ModuleBindings::element_reads`).
//!
//! Every receiver of such a read is a program Array that keeps its birth
//! length for the whole program: no store, builtin, `defineProperty` or
//! escape moves it, and no named property is ever written on it. Its
//! storage therefore never grows, so it is never forwarded
//! (`GC_FLAG_FORWARDED` comes only from growth), it holds at least
//! `FactsElementRead::len` slots, and no slot carries a descriptor. The
//! value is a pointer to that Array: no band, type or forwarding test is
//! needed, only the slot.
//!
//! A slot holds its own data or a hole. A hole reads through the prototype
//! chain, an index at or past `len` is absent: both, and in a number
//! context an element that is not a plain double, take the shared cold
//! read with the exact arguments, the generic `Get`. The proof names no
//! runtime state, so there is nothing to invalidate.

use crate::codegen::FactsElementRead;
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I32, I64, I8};

use super::{typed_feedback_emission_enabled, FnCtx};

/// The wpfacts proof for the read at `element_offset`, when the facts
/// cover this function and the build emits no observing guards.
pub(super) fn fact(ctx: &FnCtx<'_>, element_offset: u32) -> Option<FactsElementRead> {
    if element_offset == 0 || ctx.wpfacts_off || typed_feedback_emission_enabled() {
        return None;
    }
    ctx.wpfacts.element_reads.get(&element_offset).copied()
}

/// Read `arr_box[idx_i32]` under `fact`. `numeric`: the consumer is a
/// number context, whose hit is a plain double (the guarded numeric tier's
/// contract) and whose cold read coerces.
pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
    fact: FactsElementRead,
    numeric: bool,
) -> String {
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_OTHER_TIER);
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::ElementRead,
        || serde_json::json!({ "length": fact.len, "key_in_bounds": fact.key_in_bounds }),
    );
    crate::facts_consumed::used();
    if crate::verify_facts::enabled() {
        witness(ctx, arr_box, idx_i32, fact);
    }
    let load_idx = ctx.new_block("parr.load");
    let cold_idx = ctx.new_block("parr.cold");
    let merge_idx = ctx.new_block("parr.merge");
    let load_label = ctx.block_label(load_idx);
    let cold_label = ctx.block_label(cold_idx);
    let merge_label = ctx.block_label(merge_idx);
    if fact.key_in_bounds {
        ctx.block().br(&load_label);
    } else {
        let len = fact.len.to_string();
        let inside = ctx.block().icmp_ult(I32, idx_i32, &len);
        ctx.block().cond_br(&inside, &load_label, &cold_label);
    }

    ctx.current_block = load_idx;
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(arr_box);
    let handle = blk.and(I64, &bits, POINTER_MASK_I64);
    let ptr = blk.inttoptr(I64, &handle);
    let cap_ptr = blk.gep(I8, &ptr, &[(I64, "4")]);
    let capacity = blk.load(I32, &cap_ptr);
    let addr = blk.array_element_addr_with_capacity(&handle, &capacity, idx_i32);
    let slot = blk.inttoptr(I64, &addr);
    let raw = blk.load(I64, &slot);
    let value = blk.bitcast_i64_to_double(&raw);
    let hit = if numeric {
        crate::codegen::emit_plain_number_bits_test(blk, &raw)
    } else {
        blk.icmp_ne(I64, &raw, crate::nanbox::TAG_HOLE_I64)
    };
    let load_end = blk.label.clone();
    blk.cond_br(&hit, &merge_label, &cold_label);

    ctx.current_block = cold_idx;
    let idx_box = ctx.block().sitofp(I32, idx_i32, DOUBLE);
    // Mode 2: the exact arguments, no absence proof (`emit_read_cold`).
    let cold = super::guarded_array::emit_read_cold(
        ctx, arr_box, "2", &idx_box, "0", numeric, numeric, None,
    );
    let cold_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block()
        .phi(DOUBLE, &[(&value, &load_end), (&cold, &cold_end)])
}

/// `PERRY_VERIFY_FACTS=1`: the claim, tested before the proven load. The
/// value is an ordinary heap Array, not forwarded, without element
/// descriptors, of exactly `len` elements, and the key is below `len` when
/// the facts say so. A miss reports the false proof and exits 86.
fn witness(ctx: &mut FnCtx<'_>, arr_box: &str, idx_i32: &str, fact: FactsElementRead) {
    use super::guarded_array::{
        emit_array_guard_word, emit_array_guard_word_ok, HEAP_POINTER_BAND_BASE_I64,
        HEAP_POINTER_BAND_SPAN_I64,
    };
    let deref = ctx.new_block("parr.verify_deref");
    let miss = ctx.new_block("parr.verify_miss");
    let next = ctx.new_block("parr.verify_admitted");
    let (deref_l, miss_l, next_l) = (
        ctx.block_label(deref),
        ctx.block_label(miss),
        ctx.block_label(next),
    );
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(arr_box);
    let offset = blk.sub(I64, &bits, HEAP_POINTER_BAND_BASE_I64);
    let heap = blk.icmp_ult(I64, &offset, HEAP_POINTER_BAND_SPAN_I64);
    blk.cond_br(&heap, &deref_l, &miss_l);
    ctx.current_block = deref;
    let blk = ctx.block();
    let handle = blk.and(I64, &bits, POINTER_MASK_I64);
    let word = emit_array_guard_word(blk, &handle);
    let ordinary = emit_array_guard_word_ok(blk, &word, &handle);
    let ptr = blk.inttoptr(I64, &handle);
    let length = blk.load(I32, &ptr);
    let exact = blk.icmp_eq(I32, &length, &fact.len.to_string());
    let mut ok = blk.and(crate::types::I1, &ordinary, &exact);
    if fact.key_in_bounds {
        let inside = blk.icmp_ult(I32, idx_i32, &fact.len.to_string());
        ok = blk.and(crate::types::I1, &ok, &inside);
    }
    crate::verify_facts::tick(ctx);
    ctx.block().cond_br(&ok, &next_l, &miss_l);
    ctx.current_block = miss;
    crate::verify_facts::covered(
        crate::verify_facts::Fact::ElementRead,
        || serde_json::json!({ "length": fact.len, "key_in_bounds": fact.key_in_bounds }),
    );
    let site = crate::verify_facts::site_arg(
        ctx,
        &format!("proven element read of length {}", fact.len),
        crate::verify_facts::Fact::ElementRead,
        &[],
    );
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, arr_box)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
}
