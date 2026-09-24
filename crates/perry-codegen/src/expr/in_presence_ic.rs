//! Presence inline cache for `"k" in o` with a constant key.
//!
//! `in` was the last common operator lowering with no cache slot at all: every
//! `"k" in o` called `js_in_operator`, which re-derived the receiver's keys
//! array from its ShapeId (a shape-slab probe) and re-scanned it — ~950
//! instructions for a hit on a plain object, against ~15 for a property read
//! of the same key through the property PIC.
//!
//! The answer is a property of the shape, not of the object, so the site
//! caches one ShapeId and the guard below answers `true` when the receiver
//! still carries it. See `perry-runtime`'s `has_property_ic` module for what
//! the cached claim means, why only positives are cached (a negative is a
//! claim about the whole prototype chain, and there is no chain epoch to key
//! one on), and why every way of losing the key moves the receiver off this
//! guard.
//!
//! The inline path can only produce `true`. Anything the guard cannot settle —
//! a primitive receiver (which must still throw), a non-object heap value, a
//! descriptor-bearing or tombstoned object, a different shape, an unprimed
//! site — takes the same `js_in_operator` semantics through the priming entry.

use crate::nanbox::TAG_TRUE_I64;
use crate::types::{DOUBLE, I1, I16, I32, I64, I8, PTR};

use super::FnCtx;

/// Runtime `GC_TYPE_OBJECT`.
const GC_TYPE_OBJECT: &str = "2";
/// Runtime `GC_FLAG_FORWARDED` (0x80).
const GC_FLAG_FORWARDED: &str = "128";
/// `OBJ_FLAG_STABLE_TOMBSTONES | OBJ_FLAG_HAS_DESCRIPTORS` (0x400 | 0x800).
///
/// Both are read from the `_reserved` half-word the other inline guards
/// already load. A tombstoned receiver keeps its ShapeId across a `delete`
/// (#9064), so rejecting the bit is what makes the cached positive safe; a
/// descriptor-bearing one answers `in` from the accessor side table, which the
/// shape does not describe.
const IN_PIC_BLOCKING_FLAGS: &str = "3072";

/// Emit the presence guard for one `"k" in o` site, returning the NaN-boxed
/// result. `key_box` is a constant string: the cache records a ShapeId only,
/// so the key it stands for must be fixed at this site.
pub(crate) fn lower_in_presence_ic(ctx: &mut FnCtx<'_>, obj_box: &str, key_box: &str) -> String {
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache_name = super::inline_cache_global_name(ctx, site_id);
    ctx.ic_globals.push(cache_name.clone());

    let guard_idx = ctx.new_block("in.pic.guard");
    let hit_idx = ctx.new_block("in.pic.hit");
    let miss_idx = ctx.new_block("in.pic.miss");
    let merge_idx = ctx.new_block("in.pic.merge");
    let guard_label = ctx.block_label(guard_idx);
    let hit_label = ctx.block_label(hit_idx);
    let miss_label = ctx.block_label(miss_idx);
    let merge_label = ctx.block_label(merge_idx);

    // #9708: the cache sits behind a pointer slot that stays null until the
    // site's first prime. The guard block reads word 0 through the loaded
    // pointer, so the non-null test joins the receiver predicate here rather
    // than costing a branch of its own.
    let ic_slot = super::emit_inline_cache_slot(ctx, &cache_name);
    let cache_ref = ic_slot.cache.clone();
    let cache_slot_ref = ic_slot.slot_ref.clone();

    // Branch before the first header load: a primitive, a forged non-pointer
    // bit pattern and a handle-band id must never be dereferenced here. They
    // all take the miss, where `js_in_operator`'s own classification decides
    // between an answer and the TypeError ECMA-262 13.10.1 step 5 requires.
    let obj_bits = ctx.block().bitcast_double_to_i64(obj_box);
    // POINTER tag and above the native-handle band, in ONE unsigned range
    // compare (`crate::expr::receiver_range`).
    let recv = crate::expr::receiver_range::emit_fused_receiver_test(ctx.block(), &obj_bits);
    let obj_raw = crate::expr::receiver_range::emit_handle(ctx.block(), &recv.biased);
    let eligible = ctx
        .block()
        .and(I1, &recv.is_object_pointer, &ic_slot.present);
    ctx.block().cond_br(&eligible, &guard_label, &miss_label);

    ctx.current_block = guard_idx;
    crate::expr::receiver_range::emit_route_note(
        ctx.block(),
        crate::expr::receiver_range::Route::InPresence,
    );
    let gc_type_addr = ctx.block().sub(I64, &obj_raw, "8");
    let gc_type_ptr = ctx.block().inttoptr(I64, &gc_type_addr);
    let gc_type = ctx.block().load(I8, &gc_type_ptr);
    let is_object = ctx.block().icmp_eq(I8, &gc_type, GC_TYPE_OBJECT);
    let gc_flags_addr = ctx.block().sub(I64, &obj_raw, "7");
    let gc_flags_ptr = ctx.block().inttoptr(I64, &gc_flags_addr);
    let gc_flags = ctx.block().load(I8, &gc_flags_ptr);
    let forwarded = ctx.block().and(I8, &gc_flags, GC_FLAG_FORWARDED);
    let not_forwarded = ctx.block().icmp_eq(I8, &forwarded, "0");
    let reserved_addr = ctx.block().sub(I64, &obj_raw, "6");
    let reserved_ptr = ctx.block().inttoptr(I64, &reserved_addr);
    let reserved = ctx.block().load(I16, &reserved_ptr);
    let blocked = ctx.block().and(I16, &reserved, IN_PIC_BLOCKING_FLAGS);
    let ordinary = ctx.block().icmp_eq(I16, &blocked, "0");
    // ObjectHeader offset 4: the runtime ShapeId once the object is stamped,
    // else its `parent_class_id`. ShapeIds occupy 0x8000_0000..0xC000_0000,
    // disjoint from every class id, and are never reused — so an armed word
    // can only be matched by an object carrying that exact shape, and a stale
    // one can only miss.
    let shape_addr = ctx.block().add(I64, &obj_raw, "4");
    let shape_ptr = ctx.block().inttoptr(I64, &shape_addr);
    let shape_id = ctx.block().load(I32, &shape_ptr);
    let shape_word = ctx.block().zext(I32, &shape_id, I64);
    let cached_shape_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "0")]);
    let cached_shape = ctx.block().load(I64, &cached_shape_ptr);
    // The "is this site armed?" question has no test of its own: the runtime
    // resolves this cache with word 0 already holding `IN_PRESENCE_UNARMED`
    // (`1 << 32`, above every zero-extended `+4` word), so an unarmed site
    // matches nothing. It is NOT enough that no stamped shape word is 0 — an
    // UNSTAMPED receiver's `+4` is its `parent_class_id`, which is 0 for an
    // anonymous object literal, and a site resolved by a prototype-chain
    // `true` (resolved, budget counted, never armed) used to read 0 there and
    // answer `"k" in {}` with `true`. Same flaw #10833 took off the read tower.
    let shape_matches = ctx.block().icmp_eq(I64, &shape_word, &cached_shape);
    let present = ctx.block().and(I1, &is_object, &not_forwarded);
    let present = ctx.block().and(I1, &present, &ordinary);
    let present = ctx.block().and(I1, &present, &shape_matches);
    ctx.block().cond_br(&present, &hit_label, &miss_label);

    ctx.current_block = hit_idx;
    let hit_value = ctx.block().bitcast_i64_to_double(TAG_TRUE_I64);
    let hit_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = miss_idx;
    let miss_value = ctx.block().call(
        DOUBLE,
        "js_in_operator_presence_ic",
        &[(DOUBLE, obj_box), (DOUBLE, key_box), (PTR, &cache_slot_ref)],
    );
    let miss_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block()
        .phi(DOUBLE, &[(&hit_value, &hit_end), (&miss_value, &miss_end)])
}
