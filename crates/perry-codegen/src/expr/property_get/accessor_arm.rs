//! The read site's class-accessor arm (#10498): `recv.k` where `k` is a
//! compiled class getter the receiver inherits, answered by two ShapeId
//! compares, one lane load and a direct call.
//!
//! The runtime primes the entry in the site's own cache
//! (`perry-runtime/src/object/method_site/read_holder.rs`, kind
//! `PIC_HOLDER_ACCESSOR_BIT`). Every fact the hit uses is a shape fact or the
//! lane's own value:
//!
//! * the receiver's ShapeId (the token) proves `k` is not own and names the
//!   receiver's prototype identity, hence the holder: a recorded serial, or a
//!   bare class whose registry link retires the holder's ShapeId if it is
//!   ever replaced (`class_registry::retire_displaced_decl_prototype`);
//! * the holder's ShapeId proves `k`'s slot is still an accessor lane;
//! * the lane still holds the primed pair, which names the compiled getter.
//!
//! Emitted on the MRU compare's false edge, ahead of the GC-leaf front:
//!
//! ```text
//!   packed == PACKED_GET_EMPTY                     else FRONT
//!   c = @site ; c != null                          else FRONT
//!   (u32)c[RECV] == [recv+4]                       else FRONT
//!   c[KIND] & ACCESSOR                             else FRONT
//!   [c[OBJ]+4] == (u32)c[SHAPE]                    else FRONT
//!   [c[OBJ] + HDR + 8*(u32)c[KIND]] == POINTER_TAG | c[PAIR]
//!   && no worker && c[GETTER] != 0                 else FRONT
//!   r = c[GETTER](recv)
//! ```
//!
//! Only a site whose MRU word was never primed takes the arm: a site that
//! also reads own data keeps its misses on the front, which declines the
//! accessor kind, and the collecting slow call asks the same entry first. A
//! worker's start (`PERRY_METHOD_SITE_WORKERS_PRESENT`) sends every read to
//! the front: holder entries belong to the primary heap.

use super::super::FnCtx;
use crate::runtime_abi as abi;
use crate::types::{I1, I32, I64, I8, PTR};

/// Emit the arm starting at `entry_idx` (a fresh block the MRU compare's false
/// edge targets). Every failing test branches to `miss_label` (the front); the
/// call's result reaches `merge_label`. Returns `(value, end label)` for the
/// tower's merge phi.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_class_accessor_arm(
    ctx: &mut FnCtx<'_>,
    entry_idx: usize,
    packed_word: &str,
    packed_empty: i64,
    cache_slot_ref: &str,
    recv_biased: &str,
    recv_box: &str,
    header_bytes: i64,
    miss_label: &str,
    merge_label: &str,
) -> (String, String) {
    let cache_idx = ctx.new_block("pic.acc.cache");
    let recv_idx = ctx.new_block("pic.acc.recv");
    let kind_idx = ctx.new_block("pic.acc.kind");
    let holder_idx = ctx.new_block("pic.acc.holder");
    let lane_idx = ctx.new_block("pic.acc.lane");
    let call_idx = ctx.new_block("pic.acc.call");
    let cache_l = ctx.block_label(cache_idx);
    let recv_l = ctx.block_label(recv_idx);
    let kind_l = ctx.block_label(kind_idx);
    let holder_l = ctx.block_label(holder_idx);
    let lane_l = ctx.block_label(lane_idx);
    let call_l = ctx.block_label(call_idx);

    // A site that reads own data has a primed MRU word; its accessor reads
    // stay on the front and the slow call.
    ctx.current_block = entry_idx;
    {
        let blk = ctx.block();
        let empty = blk.icmp_eq(I64, packed_word, &packed_empty.to_string());
        blk.cond_br(&empty, &cache_l, miss_label);
    }

    // The full cache is allocated lazily; it is published with release order.
    ctx.current_block = cache_idx;
    let cache = {
        let blk = ctx.block();
        let cache = blk.load_atomic_acquire(PTR, cache_slot_ref, 8);
        let present = blk.icmp_ne(PTR, &cache, "null");
        blk.cond_br(&present, &recv_l, miss_label);
        cache
    };
    let word = |ctx: &mut FnCtx<'_>, index: usize| -> String {
        let blk = ctx.block();
        let p = blk.gep(I64, &cache, &[(I64, &index.to_string())]);
        blk.load(I64, &p)
    };

    // The receiver token against the receiver's ShapeId, re-read here so the
    // hot compare's load keeps its single use. A token is
    // `PIC_ID_TOKEN_BIT | ShapeId` and an empty entry is 0, so its low half
    // is a ShapeId or 0; a receiver word equal to a ShapeId proves a live
    // ordinary object of that shape (#10828 rule 3), and 0 is never one.
    ctx.current_block = recv_idx;
    let recv_word = word(ctx, abi::PIC_HOLDER_RECV_WORD);
    {
        let blk = ctx.block();
        let sid_ptr = crate::expr::receiver_range::emit_field_ptr(blk, recv_biased, 4);
        let sid = blk.load(I32, &sid_ptr);
        let primed = blk.trunc(I64, &recv_word, I32);
        let same = blk.icmp_eq(I32, &sid, &primed);
        blk.cond_br(&same, &kind_l, miss_label);
    }

    ctx.current_block = kind_idx;
    let kind = word(ctx, abi::PIC_HOLDER_KIND_WORD);
    {
        let blk = ctx.block();
        let bit = blk.and(I64, &kind, &abi::PIC_HOLDER_ACCESSOR_BIT.to_string());
        let accessor = blk.icmp_ne(I64, &bit, "0");
        blk.cond_br(&accessor, &holder_l, miss_label);
    }

    // The holder's ShapeId against the primed one.
    ctx.current_block = holder_idx;
    let holder = word(ctx, abi::PIC_HOLDER_OBJ_WORD);
    let holder_shape = word(ctx, abi::PIC_HOLDER_SHAPE_WORD);
    {
        let blk = ctx.block();
        let sid_addr = blk.add(I64, &holder, "4");
        let sid_ptr = blk.inttoptr(I64, &sid_addr);
        let sid = blk.load(I32, &sid_ptr);
        let primed = blk.trunc(I64, &holder_shape, I32);
        let same = blk.icmp_eq(I32, &sid, &primed);
        blk.cond_br(&same, &lane_l, miss_label);
    }

    // The lane still holds the primed pair; no worker; a getter to call.
    ctx.current_block = lane_idx;
    let pair = word(ctx, abi::PIC_HOLDER_PAIR_WORD);
    let getter = word(ctx, abi::PIC_HOLDER_GETTER_WORD);
    {
        let blk = ctx.block();
        let slot = blk.and(I64, &kind, "4294967295");
        let base_addr = blk.add(I64, &holder, &header_bytes.to_string());
        let base = blk.inttoptr(I64, &base_addr);
        let lane_ptr = blk.gep(I64, &base, &[(I64, &slot)]);
        let lane = blk.load(I64, &lane_ptr);
        let tagged = blk.or(I64, &pair, crate::nanbox::POINTER_TAG_I64);
        let same = blk.icmp_eq(I64, &lane, &tagged);
        let workers = blk.load_atomic_seq_cst(I8, "@PERRY_METHOD_SITE_WORKERS_PRESENT", 1);
        let workers = blk.zext(I8, &workers, I32);
        let no_workers = blk.icmp_eq(I32, &workers, "0");
        let has_getter = blk.icmp_ne(I64, &getter, "0");
        let ok = blk.and(I1, &same, &no_workers);
        let ok = blk.and(I1, &ok, &has_getter);
        blk.cond_br(&ok, &call_l, miss_label);
    }

    // The getter runs user code: a versioned loop records its bailout here,
    // as on the collecting slow call.
    ctx.current_block = call_idx;
    crate::expr::emit_versioned_loop_callback_deopt(ctx);
    let blk = ctx.block();
    let code = blk.inttoptr(I64, &getter);
    let value = crate::expr::body_call::emit_class_getter_call(blk, &code, recv_box);
    let end = blk.label.clone();
    blk.br(merge_label);
    (value, end)
}
