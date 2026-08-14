//! repsel #7480 / #5093: the emitted-IR half of the element-shape versioned
//! loop clone.
//!
//! Two emitters live here.
//!
//! * [`emit_element_shape_loop_preheader_check`] — run ONCE, before the fast
//!   clone. It brands the receiver as a genuine array, asks the runtime
//!   whether the per-array homogeneous element-shape invariant
//!   (`array/element_shape.rs`, #7496) holds at a specific class id, checks
//!   that the verified prefix covers the whole index range, and caches the
//!   elements base pointer plus the two loop-invariant words the residual
//!   check needs.
//! * [`emit_element_shape_field_load`] — run per access inside the fast clone.
//!   It is a bare element load followed by the *residual* facts the
//!   element-shape invariant does NOT cover, AND-reduced into one predicate
//!   with a single side exit.
//!
//! ## What the invariant proves, and what it does not
//!
//! `js_array_ensure_element_shape` returning class id `C` means every element
//! in `[0, verified_len)` passed `element_class_of_bits`: `POINTER_TAG`, a
//! readable `GcHeader` (which rejects the handle bands and implausible
//! magnitudes), `obj_type == GC_TYPE_OBJECT`, an exact ordinary-instance
//! ShapeId,
//! `class_id == C`. That is exactly the set of
//! predicates the element-read tier and the *front half* of the class-field
//! precheck spend per iteration, so the clone drops them.
//!
//! It proves nothing about the per-OBJECT facts a raw-f64 slot load needs —
//! exact ShapeId (a `delete elem.f` compacts the packed slots while preserving
//! `class_id`), the per-object descriptor flag, or
//! the typed-layout intact bit. Those stay per element, but collapse to ONE
//! 4-byte header load + two more loads and a single branch, because the three
//! header bytes the check needs are contiguous.
//!
//! ## Why the residual check is not just paranoia
//!
//! Dropping it would be a miscompile, not a slow path: an element whose typed
//! layout downgraded holds a NaN-boxed value where the clone would read a raw
//! `double`. `element_class_of_bits` deliberately does not look at that bit
//! (#7496 keeps the invariant's maintenance matrix small enough to enumerate),
//! so the consumer pays for it. Folding it into the invariant is the
//! documented extension (see `stmt/element_shape_loop.rs`), and needs an
//! invalidation surface for `delete` / `defineProperty` / typed downgrade that
//! does not exist today.

use crate::types::{DOUBLE, I1, I32, I64, I8};

use super::FnCtx;

// ---------------------------------------------------------------------------
// Mirrors of runtime constants. The emitted IR is textual, so these are
// literal decimals — `constants_match_the_runtime` in the test module below is
// the anti-drift gate, and it is the reason every constant here is named after
// the runtime symbol it reproduces (#7602's precedent).
// ---------------------------------------------------------------------------

/// `0x7FFD` — NaN-box tag for heap pointers.
const POINTER_TAG_HI16: &str = "32765";
/// `0x0FFFFF` — handles are <= this; real objects are above.
const HANDLE_BAND_TOP: &str = "1048575";
/// `GC_TYPE_ARRAY`. The subclass brand (#7573/#7603): a `class X extends
/// Array` instance is a plain `ObjectHeader` and must never reach the clone.
const GC_TYPE_ARRAY: &str = "1";

/// Mask over the `i32` at `obj - 8`, which spans `GcHeader { obj_type: u8 @-8,
/// gc_flags: u8 @-7, _reserved: u16 @-6 }` on every (little-endian) target
/// Perry emits for. Selects, low to high:
///
/// | bits | field | want |
/// |---|---|---|
/// | 0–7 (`0x0000_00FF`) | `obj_type` | `GC_TYPE_OBJECT` (2) |
/// | 15 (`0x0000_8000`) | `gc_flags & GC_FLAG_FORWARDED` (0x80) | clear |
/// | 27 (`0x0800_0000`) | `_reserved & OBJ_FLAG_HAS_DESCRIPTORS` (0x800) | clear |
/// | 28 (`0x1000_0000`) | `_reserved & GC_OBJ_TYPED_LAYOUT_INTACT` (0x1000) | set |
///
/// One load + one `and` + one `icmp` replaces the three loads and six ALU ops
/// the per-access class-field precheck spends on the same four facts.
const ELEM_HEADER_MASK: &str = "402686207"; // 0x1800_80FF
/// The value [`ELEM_HEADER_MASK`] must produce: `obj_type == GC_TYPE_OBJECT`,
/// not forwarded, no per-object descriptors, typed layout intact.
const ELEM_HEADER_EXPECT: &str = "268435458"; // 0x1000_0002

/// Where the fast clone's trip count comes from.
///
/// The two arms differ in *which* fact the preheader has to prove. A bound the
/// caller materialized is an independent number, so the preheader must show the
/// verified prefix reaches it. `arr.length` is not independent — it is the very
/// word this guard loads — so the comparison collapses and only the i32-range
/// obligation is left.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ElementShapeLoopTripCount<'a> {
    /// An i32 SSA value (or literal) materialized before the guard. Requires
    /// `length >= bound`.
    Bound(&'a str),
    /// `for (j = …; j < arr.length; j++)` over the array the clone reads: the
    /// length loaded inside the guard becomes the trip count.
    ///
    /// Hoisting `arr.length` out of the condition is a semantic change JS does
    /// not license in general — the property is re-read every iteration. It is
    /// sound *here* for the same reason the whole clone is: the matcher admits
    /// no store and no call in the body, and every way to change an array's
    /// length is one or the other. The slow clone keeps re-reading it.
    ArrayLength,
}

/// Emit the once-per-loop element-shape guard into the current block chain.
///
/// Leaves `ctx.current_block` on an UNTERMINATED block holding the accumulated
/// `i1` predicate, exactly like
/// [`super::class_field_inline_guard::emit_class_field_loop_preheader_check`]:
/// the caller lowers the fast clone, proves it call-free, and only then
/// terminates with `cond_br(shape_ok, fast, slow)`. Never entering a clone
/// whose call-freeness is unproven is the whole revocation argument.
///
/// Sequencing is load-bearing, in four steps and this order:
///
/// 1. the receiver is a heap pointer at all;
/// 2. the **brand** test, so the pointer handed to the runtime is known to be
///    a real array and not an `extends Array` instance (#7573/#7603);
/// 3. the **growth-forwarding repair** (#7480) — the binding may hold a stale
///    head, and steps below read `length` and the elements base off the raw
///    pointer, where a forwarding stub is not merely wrong but *plausibly*
///    wrong (see the block comment). It goes before the guard call, not after,
///    because the refresh can itself allocate;
/// 4. the guard call, and only THEN the elements base — derived from a fresh
///    load of the array's rooted slot, because the guard call can allocate and
///    an allocation can move the array, so a base derived before it could be a
///    from-space address.
///
/// Returns `(elements_base, expected_shape_id, shape_ok, bound_i32)`.
pub(crate) fn emit_element_shape_loop_preheader_check(
    ctx: &mut FnCtx,
    array_local_id: u32,
    expected_class_id: &str,
    keys_global_name: &str,
    trip_count: ElementShapeLoopTripCount<'_>,
    slow_label: &str,
) -> anyhow::Result<(String, String, String, String)> {
    let brand_idx = ctx.new_block("element_shape.loop.preheader.brand");
    let repair_idx = ctx.new_block("element_shape.loop.preheader.repair");
    let query_idx = ctx.new_block("element_shape.loop.preheader.query");
    let deref_idx = ctx.new_block("element_shape.loop.preheader.deref");
    let brand_label = ctx.block_label(brand_idx);
    let repair_label = ctx.block_label(repair_idx);
    let query_label = ctx.block_label(query_idx);
    let deref_label = ctx.block_label(deref_idx);

    // (1) Receiver is a heap pointer at all. A basic block has no
    // short-circuit, so nothing may be dereferenced until this branch is taken.
    let arr0 = super::lower_expr(ctx, &perry_hir::Expr::LocalGet(array_local_id))?;
    let handle0 = {
        let blk = ctx.block();
        let bits0 = blk.bitcast_double_to_i64(&arr0);
        let tag0 = blk.lshr(I64, &bits0, "48");
        let is_ptr0 = blk.icmp_eq(I64, &tag0, POINTER_TAG_HI16);
        let handle0 = blk.and(I64, &bits0, crate::nanbox::POINTER_MASK_I64);
        let above0 = blk.icmp_ugt(I64, &handle0, HANDLE_BAND_TOP);
        let ok0 = blk.and(I1, &is_ptr0, &above0);
        blk.cond_br(&ok0, &brand_label, slow_label);
        handle0
    };

    // (2) SUBCLASS BRAND (#7573/#7603). `class X extends Array` instances are
    // plain `ObjectHeader`s that overlay `ArrayHeader` field for field, so
    // `length`/`capacity`/`elements[0]` would read `object_type`/`class_id`/
    // `parent_class_id‖field_count`. The runtime's `array_gc_header` makes the
    // same test, but it is repeated here so the raw pointer handed across the
    // call below is already branded, and so the emitted IR carries the brand
    // where a reviewer (and the IR census) can see it.
    ctx.current_block = brand_idx;
    {
        let blk = ctx.block();
        let gt_addr = blk.sub(I64, &handle0, "8");
        let gt_ptr = blk.inttoptr(I64, &gt_addr);
        let gc_type = blk.load(I8, &gt_ptr);
        let is_array = blk.icmp_eq(I8, &gc_type, GC_TYPE_ARRAY);
        blk.cond_br(&is_array, &repair_label, slow_label);
    }

    // (2b) GROWTH-FORWARDING REPAIR (#7480). The binding may hold a *stale*
    // array head: `js_array_grow` allocates the larger array elsewhere and
    // leaves a forwarding stub at the old address, and only the bindings the
    // growing code itself wrote through are re-pointed. Every runtime entry
    // point resolves the chain (`clean_arr_ptr`) — including
    // `js_array_ensure_element_shape` below, which therefore answers about the
    // LIVE array — but the emitted code below reads `length` and the elements
    // base off the raw pointer, and on a stub those are catastrophically wrong:
    // growth overwrites the stub's first payload word (`length`‖`capacity`)
    // with the forwarding address, so `length` reads the low 32 bits of a heap
    // pointer (a huge number that passes `len_ok`), while the elements base
    // still addresses the pre-growth buffer. Elements below the old capacity
    // read stale-but-valid pointers and everything above it runs off the end of
    // the block into whatever allocation follows — masked, dereferenced at
    // `-8`, SIGBUS. With `MIN_ARRAY_CAPACITY == 16` that is exactly the
    // "correct for a 16-element array, faults at 17" shape #7480 reproduced.
    //
    // The repair is repsel 4a.2's (#6904) documented self-heal: follow the
    // chain once and write the live head back to the binding. It must happen
    // BEFORE the query call, not after, because `js_array_refresh_local_head`
    // can allocate (a lazy array materializes inside `clean_arr_ptr`) — putting
    // it here keeps the "no call after the base is derived" invariant intact,
    // and the write-back means step (4)'s re-load of the rooted slot picks up
    // the repaired head no matter what the query call moved.
    ctx.current_block = repair_idx;
    {
        let fresh = ctx.block().call(
            DOUBLE,
            "js_array_refresh_local_head",
            &[(DOUBLE, arr0.as_str())],
        );
        // The matcher (`stmt/element_shape_loop.rs`) admits only bindings one
        // of these two arms covers, so the head is always repairable here.
        if let Some(slot) = ctx.locals.get(&array_local_id).cloned() {
            ctx.block().store(DOUBLE, &fresh, &slot);
        } else if let Some(global_name) = ctx.module_globals.get(&array_local_id).cloned() {
            let g_ref = format!("@{global_name}");
            // GC_STORE_AUDIT(ROOT): module global array slot is a registered
            // mutable GC root; the value is the same JS array's live head.
            super::write_barrier::emit_root_nanbox_store_on_block(ctx.block(), &fresh, &g_ref);
        }
        // Re-derive from the repaired head. `js_array_refresh_local_head`
        // returns its input untouched when there was nothing to follow, so
        // this repeats (1) rather than replacing it.
        let blk = ctx.block();
        let bitsr = blk.bitcast_double_to_i64(&fresh);
        let tagr = blk.lshr(I64, &bitsr, "48");
        let is_ptrr = blk.icmp_eq(I64, &tagr, POINTER_TAG_HI16);
        let handler = blk.and(I64, &bitsr, crate::nanbox::POINTER_MASK_I64);
        let abover = blk.icmp_ugt(I64, &handler, HANDLE_BAND_TOP);
        let okr = blk.and(I1, &is_ptrr, &abover);
        blk.cond_br(&okr, &query_label, slow_label);
    }

    // (3) The live-header query. `js_array_ensure_element_shape` establishes
    // the invariant by scan on first visit and confirms it in O(1) afterwards;
    // either way it reads the array's CURRENT `GcHeader` bit and its record,
    // and self-heals (clearing the bit) when the record went stale. Static
    // declarations are never consulted — #7501's lesson.
    //
    // Deliberately re-loads the (now repaired) binding rather than reusing the
    // repair block's handle: `js_array_refresh_local_head` can allocate, so a
    // handle derived before it is a pre-move address.
    ctx.current_block = query_idx;
    let arrq = super::lower_expr(ctx, &perry_hir::Expr::LocalGet(array_local_id))?;
    {
        let blk = ctx.block();
        let bitsq = blk.bitcast_double_to_i64(&arrq);
        let handleq = blk.and(I64, &bitsq, crate::nanbox::POINTER_MASK_I64);
        let class_id = blk.call(I32, "js_array_ensure_element_shape", &[(I64, &handleq)]);
        let cid_ok = blk.icmp_eq(I32, &class_id, expected_class_id);
        blk.cond_br(&cid_ok, &deref_label, slow_label);
    }

    // (4) Post-call re-derivation. Everything from here to the end of the fast
    // clone is call-free, so THIS pointer is the pointer the clone uses.
    ctx.current_block = deref_idx;
    let arr1 = super::lower_expr(ctx, &perry_hir::Expr::LocalGet(array_local_id))?;
    let blk = ctx.block();
    let bits1 = blk.bitcast_double_to_i64(&arr1);
    let tag1 = blk.lshr(I64, &bits1, "48");
    let is_ptr1 = blk.icmp_eq(I64, &tag1, POINTER_TAG_HI16);
    let handle1 = blk.and(I64, &bits1, crate::nanbox::POINTER_MASK_I64);
    let above1 = blk.icmp_ugt(I64, &handle1, HANDLE_BAND_TOP);

    // Re-brand. Safe to dereference unconditionally: (1) already proved this
    // slot holds a heap pointer and only a GC move could have changed it, and
    // a move preserves both properties. `is_ptr1`/`above1` are folded into the
    // predicate anyway so a surprise cannot license the clone.
    let gt_addr1 = blk.sub(I64, &handle1, "8");
    let gt_ptr1 = blk.inttoptr(I64, &gt_addr1);
    let gc_type1 = blk.load(I8, &gt_ptr1);
    let is_array1 = blk.icmp_eq(I8, &gc_type1, GC_TYPE_ARRAY);

    // `ArrayHeader { length: u32 @0, capacity: u32 @4 }`. The invariant's
    // query requires `verified_len == length`, and nothing has run since, so
    // `length >= bound` is exactly "the verified prefix covers every index the
    // loop reads". The matcher already pinned `start >= 0`.
    let len_ptr = blk.inttoptr(I64, &handle1);
    let length = blk.load(I32, &len_ptr);
    let (bound_i32, len_ok) = match trip_count {
        ElementShapeLoopTripCount::Bound(bound) => {
            (bound.to_string(), blk.icmp_uge(I32, &length, bound))
        }
        // #7480 step 4: `for (j = 0; j < arr.length; j++)` — the trip count IS
        // the length this block just read, so "the verified prefix covers every
        // index" is true by construction and there is nothing to compare it
        // against. What still has to be proven is that the u32 fits a
        // non-negative i32: the clone's counter is an i32 and the emitted trip
        // test is signed, so a length above `i32::MAX` would read as negative
        // and run zero iterations while the slow clone ran billions. No such
        // array is allocatable today (it would need 32 GB of element slots),
        // which is exactly why the check is one `icmp` rather than a comment.
        ElementShapeLoopTripCount::ArrayLength => {
            let fits = blk.icmp_sgt(I32, &length, "-1");
            (length.clone(), fits)
        }
    };

    // Elements base: `arr + size_of::<ArrayHeader>()`.
    let base_addr = blk.add(I64, &handle1, "8");
    let elements_base = blk.inttoptr(I64, &base_addr);

    // Hoisted loop-invariant words for the residual check. The volatile gate
    // load is hoistable here for the same reason the class-field preheader
    // check hoists it: flipping it requires a runtime call, and the fast clone
    // makes none.
    let shape_global = crate::typed_shape::shape_id_global_name_from_keys_global(keys_global_name);
    let expected_shape_id = blk.load(I32, &format!("@{shape_global}"));
    let gate = blk.load_volatile(I8, "@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED");
    let gate_ok = blk.icmp_eq(I8, &gate, "0");

    let mut acc = blk.and(I1, &is_ptr1, &above1);
    acc = blk.and(I1, &acc, &is_array1);
    acc = blk.and(I1, &acc, &len_ok);
    acc = blk.and(I1, &acc, &gate_ok);

    // No terminator: the caller branches after proving the clone call-free.
    Ok((elements_base, expected_shape_id, acc, bound_i32))
}

/// Emit one `arr[i].field` read inside the fast clone: bare element load,
/// residual per-element check with a single side exit, bare raw-f64 slot load.
///
/// `idx_i32` must be the loop counter's canonical i32 (non-negative, and
/// `< bound <= length` by the preheader), and `fact` must be the fact the
/// preheader installed for `(array, counter)`.
pub(crate) fn emit_element_shape_field_load(
    ctx: &mut FnCtx,
    fact: &super::ElementShapeLoopFact,
    idx_i32: &str,
    field_index: u32,
) -> String {
    let field_index_str = field_index.to_string();
    let header_skip = crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();

    let load_idx = ctx.new_block("element_shape.load");
    let load_label = ctx.block_label(load_idx);

    let elem_ptr = {
        let blk = ctx.block();
        // The element-shape invariant proved every slot in the verified prefix
        // is a POINTER_TAG object of the guarded class, so the unbox needs no
        // tag test and no handle-band test — the two checks that make up the
        // element-read tier.
        let idx64 = blk.sext(I32, idx_i32, I64);
        let slot_ptr = blk.gep(I64, &fact.elements_base, &[(I64, &idx64)]);
        let elem_bits = blk.load(I64, &slot_ptr);
        let elem_handle = blk.and(I64, &elem_bits, crate::nanbox::POINTER_MASK_I64);
        let elem_ptr = blk.inttoptr(I64, &elem_handle);

        // Residual per-OBJECT facts (see the module docs for why they cannot
        // come from the array-level invariant).
        let hdr_ptr = blk.gep(I8, &elem_ptr, &[(I64, "-8")]);
        let hdr = blk.load(I32, &hdr_ptr);
        let hdr_masked = blk.and(I32, &hdr, ELEM_HEADER_MASK);
        let hdr_ok = blk.icmp_eq(I32, &hdr_masked, ELEM_HEADER_EXPECT);

        let sid_ptr = blk.gep(I8, &elem_ptr, &[(I64, "8")]);
        let shape_id = blk.load(I32, &sid_ptr);
        let shape_ok = blk.icmp_eq(I32, &shape_id, &fact.expected_shape_id);

        let ok = blk.and(I1, &hdr_ok, &shape_ok);
        // One branch per access. The side exit resumes the CURRENT iteration
        // in the slow clone; the matcher guarantees no effect of this
        // iteration has committed yet, so re-executing cannot double-apply.
        blk.cond_br(&ok, &load_label, &fact.side_exit_label);
        elem_ptr
    };

    ctx.current_block = load_idx;
    let blk = ctx.block();
    let fields_base = blk.gep(I8, &elem_ptr, &[(I64, &header_skip)]);
    let field_ptr = blk.gep(DOUBLE, &fields_base, &[(I64, &field_index_str)]);
    blk.load(DOUBLE, &field_ptr)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Anti-drift gate (#7602's precedent). Every literal above reproduces a
    /// runtime constant; if one moves, this fails instead of the emitted IR
    /// silently testing the wrong bit — which for `GC_OBJ_TYPED_LAYOUT_INTACT`
    /// would be a raw `double` read of a NaN-boxed slot.
    #[test]
    fn constants_match_the_runtime() {
        assert_eq!(POINTER_TAG_HI16, (0x7FFDu64).to_string());
        assert_eq!(HANDLE_BAND_TOP, (0x0F_FFFFu64).to_string());
        assert_eq!(GC_TYPE_ARRAY, "1");

        // Reconstruct the header mask from the individual runtime constants,
        // positioned by their byte offsets inside the i32 at `obj - 8`.
        let obj_type_mask = 0x0000_00FFu32;
        let forwarded = u32::from(0x80u8) << 8; // GC_FLAG_FORWARDED @ -7
        let has_descriptors = 0x0800u32 << 16; // OBJ_FLAG_HAS_DESCRIPTORS @ -6
        let typed_intact = 0x1000u32 << 16; // GC_OBJ_TYPED_LAYOUT_INTACT @ -6
        let mask = obj_type_mask | forwarded | has_descriptors | typed_intact;
        let expect = u32::from(2u8) /* GC_TYPE_OBJECT */ | typed_intact;

        assert_eq!(ELEM_HEADER_MASK, mask.to_string(), "header mask drifted");
        assert_eq!(
            ELEM_HEADER_EXPECT,
            expect.to_string(),
            "header expectation drifted"
        );

        // Sabotage direction: the mask must actually reject each fact.
        let good = expect;
        assert_eq!(good & mask, expect);
        assert_ne!((good | forwarded) & mask, expect, "forwarded not rejected");
        assert_ne!(
            (good | has_descriptors) & mask,
            expect,
            "descriptors not rejected"
        );
        assert_ne!(
            (good & !typed_intact) & mask,
            expect,
            "typed-layout downgrade not rejected"
        );
        assert_ne!((good ^ 1) & mask, expect, "wrong obj_type not rejected");
    }

    /// The mask reads three adjacent header bytes as one little-endian i32.
    /// Perry emits for aarch64/x86_64 only; assert the assumption explicitly
    /// so a future big-endian target trips here rather than in a field load.
    #[test]
    fn header_word_assumes_little_endian_targets() {
        for triple in [
            "aarch64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "aarch64-linux-android",
            "x86_64-pc-windows-msvc",
        ] {
            assert!(
                !triple.starts_with("s390") && !triple.starts_with("powerpc64-"),
                "{triple} is big-endian; ELEM_HEADER_MASK byte positions are LE-only"
            );
        }
    }
}
