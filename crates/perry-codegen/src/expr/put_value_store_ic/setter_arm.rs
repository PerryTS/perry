//! The store site's compiled-setter arm (#10498): `recv.k = v` where `k` is a
//! compiled class setter the receiver inherits, answered by two ShapeId
//! compares, one lane load and a direct call.
//!
//! The runtime primes the entry (`perry-runtime/src/proxy/put_value/
//! setter_site.rs`) and publishes it in the ways cache's setter word as
//! `SETTER_SITE_TAG | address` of a `#[repr(C)]` record. Every fact the hit
//! uses is a shape fact or the lane's own value:
//!
//! * the receiver's ShapeId proves `k` is not own and names the receiver's
//!   prototype identity, hence the holder (a recorded serial, or a bare class
//!   whose registry link retires the holder's ShapeId if it is ever replaced);
//! * the holder's ShapeId proves `k`'s slot is still an accessor lane;
//! * the lane still holds the primed pair, which names the compiled setter.
//!
//! Emitted on the shape compare's false edge, ahead of the key-add memo and
//! the existing-key ways:
//!
//! ```text
//!   packed == PACKED_SET_EMPTY                       else ADD
//!   c = @site ; c != null                            else ADD
//!   w = c[SETTER] ; w & ~MASK == SETTER_SITE_TAG     else ADD
//!   e = w & MASK ; e.recv_shape == [recv+4]          else ADD
//!   [e.holder+4] == e.holder_shape                   else ADD
//!   [e.holder + HDR + 8*e.slot] == POINTER_TAG | e.pair
//!   && no worker && v is a Number                    else ADD
//!   e.code(recv, v) ; the store's value is v
//! ```
//!
//! A non-Number value takes the miss entry, which roots it across the setter
//! (the assignment's value is the stored value, and a heap value can move).

use super::super::FnCtx;
use crate::runtime_abi as abi;
use crate::types::{I1, I32, I64, I8, PTR};

/// Emit the arm starting at `entry_idx` (a fresh block the shape compare's
/// false edge targets). Every failing test branches to `next_label` (the
/// key-add check); the call reaches `merge_label`. Returns the label of the
/// block that branches to the merge (its value is `value_double`).
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_setter_arm(
    ctx: &mut FnCtx<'_>,
    entry_idx: usize,
    packed_word: &str,
    packed_empty: i64,
    cache_slot_ref: &str,
    sid: &str,
    obj_box: &str,
    value_double: &str,
    value_bits: &str,
    header_bytes: i64,
    next_label: &str,
    merge_label: &str,
) -> String {
    let cache_idx = ctx.new_block("put.acc.cache");
    let tag_idx = ctx.new_block("put.acc.entry");
    let recv_idx = ctx.new_block("put.acc.recv");
    let holder_idx = ctx.new_block("put.acc.holder");
    let lane_idx = ctx.new_block("put.acc.lane");
    let call_idx = ctx.new_block("put.acc.call");
    let cache_l = ctx.block_label(cache_idx);
    let tag_l = ctx.block_label(tag_idx);
    let recv_l = ctx.block_label(recv_idx);
    let holder_l = ctx.block_label(holder_idx);
    let lane_l = ctx.block_label(lane_idx);
    let call_l = ctx.block_label(call_idx);

    // A site with own-data or key-add traffic keeps its misses on the ways
    // and the miss entry, which asks the same entry first.
    ctx.current_block = entry_idx;
    {
        let blk = ctx.block();
        let empty = blk.icmp_eq(I64, packed_word, &packed_empty.to_string());
        blk.cond_br(&empty, &cache_l, next_label);
    }

    ctx.current_block = cache_idx;
    let cache = {
        let blk = ctx.block();
        let cache = blk.load_atomic_acquire(PTR, cache_slot_ref, 8);
        let present = blk.icmp_ne(PTR, &cache, "null");
        blk.cond_br(&present, &tag_l, next_label);
        cache
    };

    ctx.current_block = tag_idx;
    let entry = {
        let blk = ctx.block();
        let word_ptr = blk.gep(
            I64,
            &cache,
            &[(I64, &abi::PACKED_SET_SETTER_WORD.to_string())],
        );
        let word = blk.load_atomic_monotonic(I64, &word_ptr, 8);
        let tag = blk.and(
            I64,
            &word,
            &(!abi::SETTER_SITE_ADDRESS_MASK as i64).to_string(),
        );
        let tagged = blk.icmp_eq(I64, &tag, &(abi::SETTER_SITE_TAG as i64).to_string());
        let addr = blk.and(
            I64,
            &word,
            &(abi::SETTER_SITE_ADDRESS_MASK as i64).to_string(),
        );
        blk.cond_br(&tagged, &recv_l, next_label);
        addr
    };
    // The entry's address fields are `usize`: 4 bytes on an ILP32 target, whose
    // layout differs from the host's (`abi::setter_site_layout`).
    let ilp32 = crate::target_layout::target_is_ilp32(ctx.target_triple);
    let layout = abi::setter_site_layout(if ilp32 { 4 } else { 8 });
    let field = |ctx: &mut FnCtx<'_>, offset: usize, ty| -> String {
        let blk = ctx.block();
        let a = blk.add(I64, &entry, &offset.to_string());
        let p = blk.inttoptr(I64, &a);
        blk.load(ty, &p)
    };
    // A pointer-sized field, widened to i64.
    let addr_field = |ctx: &mut FnCtx<'_>, offset: usize| -> String {
        if ilp32 {
            let narrow = field(ctx, offset, I32);
            ctx.block().zext(I32, &narrow, I64)
        } else {
            field(ctx, offset, I64)
        }
    };

    ctx.current_block = recv_idx;
    let recv_shape = field(ctx, abi::SETTER_SITE_RECV_SHAPE_OFFSET, I32);
    {
        let blk = ctx.block();
        let same = blk.icmp_eq(I32, &recv_shape, sid);
        blk.cond_br(&same, &holder_l, next_label);
    }

    ctx.current_block = holder_idx;
    let holder = addr_field(ctx, layout.holder);
    let holder_shape = field(ctx, abi::SETTER_SITE_HOLDER_SHAPE_OFFSET, I32);
    {
        let blk = ctx.block();
        let sid_addr = blk.add(I64, &holder, "4");
        let sid_ptr = blk.inttoptr(I64, &sid_addr);
        let live = blk.load(I32, &sid_ptr);
        let same = blk.icmp_eq(I32, &live, &holder_shape);
        blk.cond_br(&same, &lane_l, next_label);
    }

    ctx.current_block = lane_idx;
    let slot = field(ctx, layout.slot, I32);
    let pair = addr_field(ctx, layout.pair);
    let code = addr_field(ctx, layout.code);
    {
        let blk = ctx.block();
        let slot64 = blk.zext(I32, &slot, I64);
        let base_addr = blk.add(I64, &holder, &header_bytes.to_string());
        let base = blk.inttoptr(I64, &base_addr);
        let lane_ptr = blk.gep(I64, &base, &[(I64, &slot64)]);
        let lane = blk.load(I64, &lane_ptr);
        let tagged = blk.or(I64, &pair, crate::nanbox::POINTER_TAG_I64);
        let same = blk.icmp_eq(I64, &lane, &tagged);
        let workers = blk.load_atomic_seq_cst(I8, "@PERRY_METHOD_SITE_WORKERS_PRESENT", 1);
        let workers = blk.zext(I8, &workers, I32);
        let no_workers = blk.icmp_eq(I32, &workers, "0");
        // A Number: its top sixteen bits are outside Perry's tag band
        // 0x7FF9..=0x7FFF (`JSValue::is_number`).
        let top = blk.lshr(I64, value_bits, "48");
        let band = blk.sub(I64, &top, "32761");
        let number = blk.icmp_uge(I64, &band, "7");
        let ok = blk.and(I1, &same, &no_workers);
        let ok = blk.and(I1, &ok, &number);
        blk.cond_br(&ok, &call_l, next_label);
    }

    // The setter runs user code: a versioned loop records its bailout here,
    // as on the collecting miss entry.
    ctx.current_block = call_idx;
    crate::expr::emit_versioned_loop_callback_deopt(ctx);
    let blk = ctx.block();
    let code_ptr = blk.inttoptr(I64, &code);
    crate::expr::body_call::emit_class_setter_call(blk, &code_ptr, obj_box, value_double);
    let end = blk.label.clone();
    blk.br(merge_label);
    end
}
