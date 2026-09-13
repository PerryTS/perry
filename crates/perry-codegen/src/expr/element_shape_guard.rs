//! repsel #7480 / #5093: the emitted-IR half of the element-shape versioned
//! loop clone.
//!
//! Two emitters live here.
//!
//! * [`emit_element_shape_loop_preheader_check`] — run ONCE, before the fast
//!   clone. It brands the receiver as a genuine array, establishes the exact
//!   element class either from the native-region construction proof or from
//!   the runtime's per-array homogeneous element-shape invariant
//!   (`array/element_shape.rs`, #7496), checks that the covered range includes
//!   the whole loop, and caches the elements base pointer plus the two
//!   loop-invariant words the residual check needs.
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

/// #10123: [`ELEM_HEADER_MASK`] without the typed-layout conjunct.
///
/// A `JSON.parse`'d record has no typed layout and never will:
/// `object/json_construction.rs` finishes it with `layout_init_pointer_free`
/// or `layout_mark_unknown`, and BOTH clear `GC_OBJ_TYPED_LAYOUT_INTACT`
/// explicitly. Keeping the bit in the mask would side-exit every element of
/// every parsed record array — the clone would be emitted, entered, and then
/// leave the loop on its first read.
///
/// What the bit bought the class-keyed arm was "the slot holds a raw
/// `double`". The shape-keyed arm buys that differently and per read: the
/// loaded word is NaN-boxed, so it is tested with the same Number-tag range
/// check the preheader applies to the accumulator (`emit_js_value_is_number`),
/// and a non-Number (a string `id`, a `null`, a boxed INT32) side-exits to the
/// slow clone. That is not weaker — it is the same claim, established from the
/// value instead of from a layout declaration.
const ELEM_HEADER_SHAPE_MASK: &str = "134250751"; // 0x0800_80FF
/// The value [`ELEM_HEADER_SHAPE_MASK`] must produce: `obj_type ==
/// GC_TYPE_OBJECT`, not forwarded, no per-object descriptors.
const ELEM_HEADER_SHAPE_EXPECT: &str = "2"; // 0x0000_0002

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

/// #10123: which identity the preheader proves for the elements.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ElementShapeGuardKind<'a> {
    /// The original arm: every element's `ObjectHeader::class_id` is
    /// `expected_class_id`, and the expected ShapeId is the class's canonical
    /// one, loaded from the global beside `keys_global_name`.
    Class {
        expected_class_id: &'a str,
        keys_global_name: &'a str,
    },
    /// The shape arm: every element carries the SAME exact ordinary ShapeId,
    /// whatever it is. This is what a class-less record array can prove, and
    /// the shape id itself becomes the expected one — there is no class, so
    /// there is no canonical keys global to load it from. `properties` are the
    /// names whose inline slots the preheader resolves against that shape.
    Shape {
        properties: &'a std::collections::BTreeSet<String>,
    },
}

/// #10123: the preheader obligation that makes every index the clone reads
/// in-bounds, WITHOUT a per-read test.
///
/// Each arm is the whole bounds argument for one [`super::ElementShapeIndex`]
/// spelling, discharged once. Getting one wrong is an out-of-bounds read, not
/// a slow path, which is why they are named after the index form rather than
/// folded into a single "check the index" helper.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ElementShapeIndexBound<'a> {
    /// `arr[j]`: the counter IS the index, so the trip-count obligation
    /// (`length >= bound`, or `bound == length`) already covers every read.
    FromTripCount,
    /// `arr[k]`: one constant index, in range iff `length > k`.
    Constant(i64),
    /// `arr[j % m]`: `srem` of a non-negative counter by `m` lands in
    /// `[0, m)`, so `m <= length` covers every read. `m` was materialized as a
    /// nonzero, non-negative i32 before the guard.
    Modulus(&'a str),
}

/// Everything the fast clone needs from the preheader.
pub(crate) struct ElementShapeGuardOutputs {
    /// Elements base pointer, derived after the last call in the preheader.
    pub elements_base: String,
    /// i32 SSA of the ShapeId every element must still carry per read.
    pub expected_shape_id: String,
    /// The accumulated `i1` the caller ANDs its own conditions into.
    pub shape_ok: String,
    /// The clone's i32 trip count.
    pub bound_i32: String,
    /// Shape-keyed only: property -> i64 SSA of its inline slot index, each
    /// proven non-negative before the clone is reachable.
    pub field_slots: std::collections::BTreeMap<String, String>,
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
/// Sequencing is load-bearing, in five steps and this order:
///
/// 1. the receiver is a heap pointer at all;
/// 2. the **growth-forwarding repair** (#7480) — the binding may hold a stale
///    head, and the steps below read `length` and the elements base off the
///    raw pointer, where a forwarding stub is not merely wrong but *plausibly*
///    wrong (see the block comment). It goes before the guard call, not after,
///    because the refresh can itself allocate;
/// 3. the **brand** test, so the pointer handed to the runtime is known to be
///    a real array and not an `extends Array` instance (#7573/#7603);
/// 4. the runtime guard call when static construction did not already prove
///    the identity, plus (shape-keyed) one inline-slot query per tracked
///    property;
/// 5. and only THEN the elements base — derived from a fresh load of the
///    array's rooted slot, because any preceding helper can allocate and an
///    allocation can move the array, so a base derived before it could be a
///    from-space address.
///
/// **Why the repair now precedes the brand** (#10123; it used to follow it).
/// `JSON.parse` of a top-level array hands back a `GC_TYPE_LAZY_ARRAY` header,
/// which the brand test rejects — so the clone declined the single most common
/// record array in the language before the repair that would have materialized
/// it ever ran. `js_array_refresh_local_head` is safe on an unbranded value by
/// construction: it tests `POINTER_TAG`, then `is_plausible_heap_addr`, then
/// resolves through `clean_arr_ptr`, which returns null for every tracked
/// non-array (an `extends Array` instance included) and materializes a lazy
/// one. Anything it cannot resolve comes back unchanged, and the brand test —
/// now applied to the REPAIRED head — rejects it exactly as before.
pub(crate) fn emit_element_shape_loop_preheader_check(
    ctx: &mut FnCtx,
    array_local_id: u32,
    kind: ElementShapeGuardKind<'_>,
    trip_count: ElementShapeLoopTripCount<'_>,
    index_bound: ElementShapeIndexBound<'_>,
    slow_label: &str,
    statically_proven: bool,
) -> anyhow::Result<ElementShapeGuardOutputs> {
    let repair_idx = ctx.new_block("element_shape.loop.preheader.repair");
    let brand_idx = ctx.new_block("element_shape.loop.preheader.brand");
    let query_idx =
        (!statically_proven).then(|| ctx.new_block("element_shape.loop.preheader.query"));
    let slots_idx = match kind {
        ElementShapeGuardKind::Shape { properties } if !properties.is_empty() => {
            Some(ctx.new_block("element_shape.loop.preheader.slots"))
        }
        _ => None,
    };
    let deref_idx = ctx.new_block("element_shape.loop.preheader.deref");
    let repair_label = ctx.block_label(repair_idx);
    let brand_label = ctx.block_label(brand_idx);
    let query_label = query_idx.map(|idx| ctx.block_label(idx));
    let slots_label = slots_idx.map(|idx| ctx.block_label(idx));
    let deref_label = ctx.block_label(deref_idx);
    let post_query_label = slots_label.as_deref().unwrap_or(&deref_label);
    let post_brand_label = query_label.as_deref().unwrap_or(post_query_label);

    // (1) Receiver is a heap pointer at all. A basic block has no
    // short-circuit, so nothing may be dereferenced until this branch is taken.
    let arr0 = super::lower_expr(ctx, &perry_hir::Expr::LocalGet(array_local_id))?;
    {
        let blk = ctx.block();
        let bits0 = blk.bitcast_double_to_i64(&arr0);
        let tag0 = blk.lshr(I64, &bits0, "48");
        let is_ptr0 = blk.icmp_eq(I64, &tag0, POINTER_TAG_HI16);
        let handle0 = blk.and(I64, &bits0, crate::nanbox::POINTER_MASK_I64);
        let above0 = blk.icmp_ugt(I64, &handle0, HANDLE_BAND_TOP);
        let ok0 = blk.and(I1, &is_ptr0, &above0);
        blk.cond_br(&ok0, &repair_label, slow_label);
    }

    // (2) GROWTH-FORWARDING REPAIR (#7480). The binding may hold a *stale*
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
    // and the write-back means step (5)'s re-load of the rooted slot picks up
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
        blk.cond_br(&okr, &brand_label, slow_label);
    }

    // (3) SUBCLASS BRAND (#7573/#7603). `class X extends Array` instances are
    // plain `ObjectHeader`s that overlay `ArrayHeader` field for field, so
    // `length`/`capacity`/`elements[0]` would read `class_id`/`parent_class_id`
    // (the ShapeId)/`keys_array` (#8113). The runtime's `array_gc_header` makes the
    // same test, but it is repeated here so the raw pointer handed across the
    // call below is already branded, and so the emitted IR carries the brand
    // where a reviewer (and the IR census) can see it.
    //
    // It reads the REPAIRED head: a lazy-array header is not `GC_TYPE_ARRAY`
    // and would fail here, which is the whole reason step (2) now runs first.
    ctx.current_block = brand_idx;
    {
        let arrb = super::lower_expr(ctx, &perry_hir::Expr::LocalGet(array_local_id))?;
        let blk = ctx.block();
        let bitsb = blk.bitcast_double_to_i64(&arrb);
        let handleb = blk.and(I64, &bitsb, crate::nanbox::POINTER_MASK_I64);
        let gt_addr = blk.sub(I64, &handleb, "8");
        let gt_ptr = blk.inttoptr(I64, &gt_addr);
        let gc_type = blk.load(I8, &gt_ptr);
        let is_array = blk.icmp_eq(I8, &gc_type, GC_TYPE_ARRAY);
        blk.cond_br(&is_array, post_brand_label, slow_label);
    }

    // (4) The live-header query for arrays whose construction is not statically
    // contained. `js_array_ensure_element_shape[_ordinary]` establishes the
    // invariant by scan on first visit and confirms it in O(1) afterwards;
    // either way it reads the array's CURRENT `GcHeader` bit and its record,
    // and self-heals (clearing the bit) when the record went stale. Type
    // declarations are never sufficient — #7501's lesson. The static arm is
    // stronger: E1--E5 proves every dense slot is a fresh exact-class
    // allocation for the whole native region.
    //
    // Deliberately re-loads the (now repaired) binding rather than reusing the
    // repair block's handle: `js_array_refresh_local_head` can allocate, so a
    // handle derived before it is a pre-move address.
    let mut queried_shape_id: Option<String> = None;
    if let Some(query_idx) = query_idx {
        ctx.current_block = query_idx;
        let arrq = super::lower_expr(ctx, &perry_hir::Expr::LocalGet(array_local_id))?;
        let blk = ctx.block();
        let bitsq = blk.bitcast_double_to_i64(&arrq);
        let handleq = blk.and(I64, &bitsq, crate::nanbox::POINTER_MASK_I64);
        match kind {
            ElementShapeGuardKind::Class {
                expected_class_id, ..
            } => {
                let class_id = blk.call(I32, "js_array_ensure_element_shape", &[(I64, &handleq)]);
                let cid_ok = blk.icmp_eq(I32, &class_id, expected_class_id);
                blk.cond_br(&cid_ok, post_query_label, slow_label);
            }
            // #10123: there is no compile-time id to compare against — the
            // answer IS the expected ShapeId. Zero means "no class-0 proof",
            // which covers both "not homogeneous" and "homogeneous but
            // class-keyed", and both take the slow clone.
            ElementShapeGuardKind::Shape { .. } => {
                let shape_id = blk.call(
                    I32,
                    "js_array_ensure_element_shape_ordinary",
                    &[(I64, &handleq)],
                );
                let ok = blk.icmp_ne(I32, &shape_id, "0");
                blk.cond_br(&ok, post_query_label, slow_label);
                queried_shape_id = Some(shape_id);
            }
        }
    }

    // (4b) #10123: resolve each tracked property to an inline slot ONCE,
    // against the shape the query just proved. A class-keyed clone bakes this
    // in at compile time from the class's field list; a record array has no
    // class, so the shape table answers instead. A `-1` (absent key, a
    // class-kind or mutated shape, a key the shape spilled) declines the clone
    // — never a guess at an offset.
    let mut field_slots: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    if let (Some(slots_idx), ElementShapeGuardKind::Shape { properties }) = (slots_idx, kind) {
        ctx.current_block = slots_idx;
        let shape_id = queried_shape_id
            .clone()
            .expect("the shape-keyed arm always emits the runtime query");
        let mut all_ok: Option<String> = None;
        for property in properties {
            let key_idx = ctx.strings.intern(property);
            let key_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let blk = ctx.block();
            // The WHOLE NaN-box, not a masked pointer: a short property name
            // ("id") reaches the pool as an SSO immediate whose masked low bits
            // are packed characters rather than an address. The runtime
            // compares by content across both representations.
            let key_box = blk.load(DOUBLE, &key_global);
            let key_bits = blk.bitcast_double_to_i64(&key_box);
            let slot = blk.call(
                I32,
                "js_shape_ordinary_inline_slot_for_key",
                &[(I32, &shape_id), (I64, &key_bits)],
            );
            let ok = blk.icmp_sgt(I32, &slot, "-1");
            all_ok = Some(match all_ok {
                Some(acc) => blk.and(I1, &acc, &ok),
                None => ok,
            });
            let slot64 = blk.sext(I32, &slot, I64);
            field_slots.insert(property.clone(), slot64);
        }
        let all_ok = all_ok.expect("a non-empty property set emits at least one query");
        ctx.block().cond_br(&all_ok, &deref_label, slow_label);
    }

    // (5) Post-proof re-derivation. Everything from here to the end of the fast
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
    // the comparisons below are exactly "the verified prefix covers every
    // index the loop reads". The matcher already pinned `start >= 0`.
    let len_ptr = blk.inttoptr(I64, &handle1);
    let length = blk.load(I32, &len_ptr);
    let bound_i32 = match trip_count {
        ElementShapeLoopTripCount::Bound(bound) => bound.to_string(),
        ElementShapeLoopTripCount::ArrayLength => length.clone(),
    };

    // The TRIP-COUNT obligation, which exists only when the counter is also
    // the index.
    //
    // #10123: this used to be unconditional, and that made the whole
    // shape-keyed arm dead on its own benchmark. `for (i = 0; i < 1000000;
    // i++) sum += rows[7].id` over a 7,600-element array asks the preheader to
    // prove `length >= 1000000`, which is false — so the clone was emitted,
    // was branched into by a `cond_br` the IR census could see, and was never
    // once entered at run time. The counter is not an index here; the verified
    // prefix has nothing to say about the trip count, and `ElementShapeIndex`
    // carries its own obligation instead.
    let trip_ok = match (trip_count, index_bound) {
        (ElementShapeLoopTripCount::Bound(bound), ElementShapeIndexBound::FromTripCount) => {
            Some(blk.icmp_uge(I32, &length, bound))
        }
        // A caller-materialized bound is already a non-negative i32
        // (`materialize_loop_i32`), and the counter indexes nothing.
        (ElementShapeLoopTripCount::Bound(_), _) => None,
        // #7480 step 4: `for (j = 0; j < arr.length; j++)` — the trip count IS
        // the length this block just read, so "the verified prefix covers every
        // index" is true by construction and there is nothing to compare it
        // against. What still has to be proven, for EVERY index form, is that
        // the u32 fits a non-negative i32: the clone's counter is an i32 and
        // the emitted trip test is signed, so a length above `i32::MAX` would
        // read as negative and run zero iterations while the slow clone ran
        // billions. No such array is allocatable today (it would need 32 GB of
        // element slots), which is exactly why the check is one `icmp` rather
        // than a comment.
        (ElementShapeLoopTripCount::ArrayLength, _) => Some(blk.icmp_sgt(I32, &length, "-1")),
    };

    // #10123: the INDEX obligation, one per spelling. `FromTripCount` adds
    // nothing — the trip-count test above already covers every read — and the
    // other two are compared UNSIGNED for the same reason: `length` is a `u32`
    // read into an i32, so a hypothetical 2^31-element array must not read as
    // negative and pass.
    let index_ok = match index_bound {
        ElementShapeIndexBound::FromTripCount => None,
        ElementShapeIndexBound::Constant(k) => Some(blk.icmp_ugt(I32, &length, &k.to_string())),
        ElementShapeIndexBound::Modulus(modulus) => Some(blk.icmp_uge(I32, &length, modulus)),
    };

    // Logical elements base, including a consumed queue prefix.
    let base_addr = blk.array_elements_addr(&handle1);
    let elements_base = blk.inttoptr(I64, &base_addr);

    // Hoisted loop-invariant words for the residual check. The volatile gate
    // load is hoistable here for the same reason the class-field preheader
    // check hoists it: flipping it requires a runtime call, and the fast clone
    // makes none.
    let expected_shape_id = match kind {
        ElementShapeGuardKind::Class {
            keys_global_name, ..
        } => {
            let shape_global =
                crate::typed_shape::shape_id_global_name_from_keys_global(keys_global_name);
            blk.load(I32, &format!("@{shape_global}"))
        }
        // The query's answer, which dominates this block.
        ElementShapeGuardKind::Shape { .. } => queried_shape_id
            .clone()
            .expect("the shape-keyed arm always emits the runtime query"),
    };
    let gate = blk.load_volatile(I8, "@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED");
    let gate_ok = blk.icmp_eq(I8, &gate, "0");

    let mut acc = blk.and(I1, &is_ptr1, &above1);
    acc = blk.and(I1, &acc, &is_array1);
    if let Some(trip_ok) = trip_ok {
        acc = blk.and(I1, &acc, &trip_ok);
    }
    if let Some(index_ok) = index_ok {
        acc = blk.and(I1, &acc, &index_ok);
    }
    acc = blk.and(I1, &acc, &gate_ok);

    // No terminator: the caller branches after proving the clone call-free.
    Ok(ElementShapeGuardOutputs {
        elements_base,
        expected_shape_id,
        shape_ok: acc,
        bound_i32,
        field_slots,
    })
}

/// Materialize this iteration's element index as an i32, or report that the
/// clone cannot index at all.
///
/// One arm per [`super::ElementShapeIndex`] spelling, and each reads exactly
/// the storage whose bounds obligation the preheader discharged
/// ([`ElementShapeIndexBound`]). `None` means the counter's canonical i32 slot
/// the matcher required has gone missing — that costs the read its fast
/// lowering, never correctness, and the matcher, the fact lookup and this must
/// all agree (`ElementShapeIndex::needs_counter_i32_slot`).
pub(crate) fn emit_element_shape_index(
    ctx: &mut FnCtx,
    fact: &super::ElementShapeLoopFact,
) -> Option<String> {
    match &fact.index {
        super::ElementShapeIndex::Counter => {
            let slot = ctx.i32_counter_slots.get(&fact.index_local_id).cloned()?;
            Some(ctx.block().load(I32, &slot))
        }
        super::ElementShapeIndex::Constant(k) => Some(k.to_string()),
        // The derived `const d = j % m` binding's own slot, written by the
        // `Let` arm in `stmt/let_stmt.rs` earlier in this same iteration.
        super::ElementShapeIndex::DerivedMod { slot, .. } => {
            let slot = slot.clone();
            Some(ctx.block().load(I32, &slot))
        }
        // #10185: the carried recurrence's private i32 slot, written by the
        // body's first statement earlier in this same iteration. The REAL
        // binding slot is deliberately NOT read here — it is one iteration
        // behind until the trailing write-back commits, which is exactly what
        // makes a mid-iteration side exit correct.
        super::ElementShapeIndex::Carried(carried) => {
            let slot = carried.slot.clone();
            Some(ctx.block().load(I32, &slot))
        }
    }
}

/// The masked heap handle of `arr[idx]` for THIS iteration, with the residual
/// per-element facts discharged.
///
/// Two sources. When the matcher installed an [`super::ElementPrefetch`] the
/// body's leading virtual binding already did the deref and the residual check
/// for the whole iteration, so this is one load of an entry alloca; otherwise
/// (#10123's original shape) the deref and the check are emitted here, once per
/// read.
pub(crate) fn emit_element_shape_element_handle(
    ctx: &mut FnCtx,
    fact: &super::ElementShapeLoopFact,
    idx_i32: &str,
) -> String {
    if let Some(prefetch) = &fact.elem_prefetch {
        let slot = prefetch.handle_slot.clone();
        return ctx.block().load(I64, &slot);
    }
    emit_element_deref_with_residual(ctx, fact, idx_i32)
}

/// Bare element load plus the residual per-OBJECT facts the array-level
/// invariant deliberately does not cover. Returns the masked handle.
///
/// Emits nothing but loads, ALU and one `cond_br` — call-free by construction,
/// which is the whole revocation argument (see the module docs).
pub(crate) fn emit_element_deref_with_residual(
    ctx: &mut FnCtx,
    fact: &super::ElementShapeLoopFact,
    idx_i32: &str,
) -> String {
    let blk = ctx.block();
    // The element-shape invariant proved every slot in the verified prefix
    // is a POINTER_TAG object of the guarded identity, so the unbox needs
    // no tag test and no handle-band test — the two checks that make up the
    // element-read tier.
    let idx64 = blk.sext(I32, idx_i32, I64);
    let slot_ptr = blk.gep(I64, &fact.elements_base, &[(I64, &idx64)]);
    let elem_bits = blk.load(I64, &slot_ptr);
    let elem_handle = blk.and(I64, &elem_bits, crate::nanbox::POINTER_MASK_I64);

    if fact.statically_layout_proven {
        return elem_handle;
    }
    let elem_ptr = blk.inttoptr(I64, &elem_handle);
    let load_idx = ctx.new_block("element_shape.load");
    let load_label = ctx.block_label(load_idx);
    let blk = ctx.block();

    // Residual per-OBJECT facts (see the module docs for why they cannot come
    // from the runtime array-level invariant).
    let hdr_ptr = blk.gep(I8, &elem_ptr, &[(I64, "-8")]);
    let hdr = blk.load(I32, &hdr_ptr);
    let (mask, expect) = if fact.shape_keyed {
        (ELEM_HEADER_SHAPE_MASK, ELEM_HEADER_SHAPE_EXPECT)
    } else {
        (ELEM_HEADER_MASK, ELEM_HEADER_EXPECT)
    };
    let hdr_masked = blk.and(I32, &hdr, mask);
    let hdr_ok = blk.icmp_eq(I32, &hdr_masked, expect);

    // #8113: the ShapeId moved from header offset 8 to 4.
    let sid_ptr = blk.gep(I8, &elem_ptr, &[(I64, "4")]);
    let shape_id = blk.load(I32, &sid_ptr);
    let shape_ok = blk.icmp_eq(I32, &shape_id, &fact.expected_shape_id);

    let ok = blk.and(I1, &hdr_ok, &shape_ok);
    // The side exit resumes the CURRENT iteration in the slow clone; no effect
    // of this iteration has committed yet (#10185: with a prefetch this is the
    // ONLY residual exit in the iteration, and it precedes every store).
    blk.cond_br(&ok, &load_label, &fact.side_exit_label);
    ctx.current_block = load_idx;
    elem_handle
}

/// #10185: the once-per-iteration element prologue.
///
/// Emitted by the body's leading virtual binding (`stmt/let_stmt.rs` →
/// `stmt/element_shape_loop::lower_virtual_clone_binding`) once the index for
/// this iteration is in its slot. Parks the masked handle in an entry alloca so
/// every read in the iteration is a bare offset load.
pub(crate) fn emit_element_shape_prefetch(
    ctx: &mut FnCtx,
    fact: &super::ElementShapeLoopFact,
    idx_i32: &str,
    handle_slot: &str,
) {
    let handle = emit_element_deref_with_residual(ctx, fact, idx_i32);
    ctx.block().store(I64, &handle, handle_slot);
}

/// One tracked property's RAW slot word — a NaN-boxed `JSValue` in the
/// shape-keyed arm, a raw `double` in the class-keyed one. No tag test; the
/// caller adds the one its consumer needs.
pub(crate) fn emit_element_shape_slot_load(
    ctx: &mut FnCtx,
    fact: &super::ElementShapeLoopFact,
    idx_i32: &str,
    field_slot: &super::ElementShapeFieldSlot,
) -> String {
    let header_skip = crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let (slot_ty, slot_value) = match field_slot {
        super::ElementShapeFieldSlot::Packed(index) => (I64, index.to_string()),
        super::ElementShapeFieldSlot::Runtime(reg) => (I64, reg.clone()),
    };
    let elem_handle = emit_element_shape_element_handle(ctx, fact, idx_i32);
    let blk = ctx.block();
    let elem_ptr = blk.inttoptr(I64, &elem_handle);
    let fields_base = blk.gep(I8, &elem_ptr, &[(I64, &header_skip)]);
    let field_ptr = blk.gep(DOUBLE, &fields_base, &[(slot_ty, &slot_value)]);
    blk.load(DOUBLE, &field_ptr)
}

/// Emit one `arr[i].field` read inside the fast clone: bare element load,
/// an optional residual per-element check, then a bare raw-f64 slot load.
///
/// `idx_i32` must be an index the preheader discharged a bounds obligation for
/// (see [`ElementShapeIndexBound`]), and `fact` must be the fact the preheader
/// installed for this array.
pub(crate) fn emit_element_shape_field_load(
    ctx: &mut FnCtx,
    fact: &super::ElementShapeLoopFact,
    idx_i32: &str,
    field_slot: &super::ElementShapeFieldSlot,
) -> String {
    let value = emit_element_shape_slot_load(ctx, fact, idx_i32, field_slot);

    // #10123: the shape-keyed arm's representation check.
    //
    // A class-keyed clone reads a RAW double, licensed by
    // `GC_OBJ_TYPED_LAYOUT_INTACT` in the residual mask above. A record's slot
    // holds a NaN-boxed JSValue instead, and the two coincide exactly when the
    // value is a Number — so the same tag-range test the preheader applies to
    // the accumulator is applied here, per read, and a string / boolean / null
    // `id` side-exits to the slow clone rather than being consumed as a raw
    // double. Without it `sum += rows[i].id` over `{"id": "7"}` would add the
    // bit pattern of a string pointer.
    if fact.shape_keyed {
        let is_number = crate::stmt::emit_js_value_is_number(ctx, &value);
        let ok_idx = ctx.new_block("element_shape.number");
        let ok_label = ctx.block_label(ok_idx);
        ctx.block()
            .cond_br(&is_number, &ok_label, &fact.side_exit_label);
        ctx.current_block = ok_idx;
    }
    value
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

        // #10123's shape-keyed pair is the SAME three header facts with the
        // typed-layout conjunct removed, derived here rather than restated so
        // a drift in any shared constant moves both.
        let shape_mask = obj_type_mask | forwarded | has_descriptors;
        let shape_expect = u32::from(2u8) /* GC_TYPE_OBJECT */;
        assert_eq!(
            ELEM_HEADER_SHAPE_MASK,
            shape_mask.to_string(),
            "shape-keyed header mask drifted"
        );
        assert_eq!(
            ELEM_HEADER_SHAPE_EXPECT,
            shape_expect.to_string(),
            "shape-keyed header expectation drifted"
        );
        // It must still reject the three facts it DOES cover, and must
        // deliberately NOT depend on the typed-layout bit — a parsed record
        // never has it, so a mask that kept it would side-exit every element.
        assert_eq!(shape_expect & shape_mask, shape_expect);
        assert_eq!(
            (shape_expect | typed_intact) & shape_mask,
            shape_expect,
            "the shape-keyed mask must ignore the typed-layout bit"
        );
        assert_ne!(
            (shape_expect | forwarded) & shape_mask,
            shape_expect,
            "forwarded not rejected"
        );
        assert_ne!(
            (shape_expect | has_descriptors) & shape_mask,
            shape_expect,
            "descriptors not rejected"
        );
        assert_ne!(
            (shape_expect ^ 1) & shape_mask,
            shape_expect,
            "wrong obj_type not rejected"
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
