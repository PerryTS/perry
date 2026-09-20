//! The inline HIT of the dynamically-typed `obj[i]` element read: four guarded
//! arms and one out-of-line exit.
//!
//! Split out of `index_get.rs` to keep that file under the 2000-line cap; the
//! entry point stays `pub(super)` so the trunk's call sites keep compiling.
//!
//! # What is inline and what is not (#T2, "inline hit, one exit")
//!
//! The site keeps four guarded arms and ONE out-of-line call. Each arm is here
//! because removing it was measured and cost runtime; each absent arm is
//! absent because keeping it was measured and cost only bytes.
//!
//!   1. receiver NaN-box tag / heap-band and canonical-index checks, then the
//!      managed `GcHeader` kind (+ forwarding) load;
//!   2. the packed ordinary `GC_TYPE_ARRAY` arm (bounds check, element load,
//!      hole -> `undefined`);
//!   3. the typed-array arm, collapsed from eight per-kind load blocks behind
//!      a seven-block kind dispatch onto the four ELEMENT WIDTHS the header
//!      already stores (`tav.w1/w2/w4/w8`), each resolving its own
//!      signedness/float form with `select`s — behind #10118's tag-only brand
//!      test, so a non-typed-array receiver still pays one `icmp` to leave;
//!   4. the elements-backed Array-subclass probe (`ObjectMeta.elements`),
//!      which is the DEFAULT representation of `class X extends Array`.
//!
//! Everything else reaches `js_packed_arraylike_index_get`, the site's single
//! non-feedback out-of-line call and the same call the old `arrlike.ic.miss`
//! block made: BigInt/Float16 lanes, a live typed-array view, the
//! lazy-JSON-array tier (which moved INTO that helper) and the whole
//! shape-carried Array-subclass IC tower.
//!
//! The tower is the large removal — fifteen blocks per site
//! (`arrlike.ic.{shape,identity,exact,family_meta,family_token,bounds,
//! length_inline,length_spill_meta,length_spill_ptr,length_spill_load,range,
//! inline,spill,spill_ptr,spill_load,spill_or_miss}`) — and it is removable
//! because it CANNOT HIT in the shipped configuration: its hit needs a primed
//! layout cache, and the only writer of that cache reaches
//! `build_dense_layout` only when `elements_of(obj)` is null, which the
//! default elements store makes false for every Array subclass. See the
//! `arrlike.elem.*` section below for the full argument.
//!
//! This used to emit ~50 basic blocks and ~393 pre-RS4GC instructions per
//! `a[i]`; on `prettier/plugins/flow.mjs` — a program that never constructs a
//! typed array and never subclasses `Array` — that tower was 55% of all
//! emitted IR across 10,778 sites, and each of its ~4 runtime calls is a
//! statepoint whose `.perry_gcmap` scales with the live GC values at the site.
//!
//! # Rooting (Layer 1, slice 4)
//!
//! Listed in `crate::rooting`'s `MIGRATED_MODULES`, and the listing is
//! **vacuous on the committed source**: this module has never named an
//! `expr::temp_root` symbol, so only the sabotage arm makes the line an
//! assertion. The audit that earned it: the entry point receives the receiver
//! and index already lowered, lowers no user expression, and emits only pure
//! IR (guards, GEPs, loads) plus an out-of-line semantic fallback, so no
//! register of a GC value spans a lowering here.

use crate::types::{DOUBLE, F32, I1, I16, I32, I64, I8, PTR};

use super::FnCtx;

/// Emit the guarded inline element read for an `obj[i]` whose receiver static
/// type is erased (`any`/unknown).
///
/// `obj_box` / `idx_d` are the already-lowered receiver and index (DOUBLE).
///
/// `coerce_slow_to_number`: when the read is used in a context that will
/// `ToNumber` the result regardless (a non-`+` arithmetic / bitwise operand —
/// `^`, `-`, `*`, `<<`, …; see [`lower_unknown_local_index_get_for_number_
/// context`]), EVERY arm wraps its result in `js_number_coerce` so the merged
/// value is always a Number and the caller can skip the per-element site
/// coerce it would otherwise emit. `false` leaves the result boxed (the
/// general `obj[i]` read, whose result may legitimately be a non-Number).
///
/// The typed-array arms never coerce: their value is a Number by
/// construction, which is the #5525 property that made inlining them worth it
/// (bcryptjs's `S[i]`/`P[i]` Blowfish boxes, ~600M reads for one cost-12
/// `compareSync`).
///
/// [`lower_unknown_local_index_get_for_number_context`]: super::lower_unknown_local_index_get_for_number_context
pub(super) fn lower_inline_dyn_typed_array_get(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    idx_d: &str,
    coerce_slow_to_number: bool,
) -> String {
    // TAG_MASK / POINTER_TAG / POINTER_MASK as signed-i64 LLVM literals.
    let tag_mask = crate::nanbox::i64_literal(crate::nanbox::TAG_MASK);
    let pointer_tag = crate::nanbox::POINTER_TAG_I64;
    let pointer_mask = crate::nanbox::POINTER_MASK_I64;

    // #9708: the site owns an 8-byte pointer SLOT (`__bss`, null until the
    // runtime primes it), not the cache words. The site no longer reads the
    // cache at all — the tier that did (the shape-carried Array-subclass IC)
    // lives behind the exit now — but it still owns the slot, because the exit
    // is handed the slot's ADDRESS, which is a link-time constant needing no
    // load.
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache_name = super::super::inline_cache_global_name(ctx, site_id);
    ctx.ic_globals.push(cache_name.clone());
    let slot_ref = format!("@{cache_name}");

    let object_header_idx = ctx.new_block("arrlike.ic.header");
    let object_brand_idx = ctx.new_block("arrlike.ic.brand");
    let object_array_guard_idx = ctx.new_block("arrlike.ic.array_guard");
    let object_array_load_idx = ctx.new_block("arrlike.ic.array_load");
    let ta_brand_idx = ctx.new_block("tav.brand");
    let ta_kind_guard_idx = ctx.new_block("tav.kind_guard");
    let ta_width_idx = ctx.new_block("tav.width");
    let ta_width4_idx = ctx.new_block("tav.width4");
    let ta_width2_idx = ctx.new_block("tav.width2");
    let ta_w8_idx = ctx.new_block("tav.w8");
    let ta_w4_idx = ctx.new_block("tav.w4");
    let ta_w2_idx = ctx.new_block("tav.w2");
    let ta_w1_idx = ctx.new_block("tav.w1");
    let elem_kind_idx = ctx.new_block("arrlike.elem.kind");
    let elem_meta_idx = ctx.new_block("arrlike.elem.meta");
    let elem_store_idx = ctx.new_block("arrlike.elem.store");
    let elem_bounds_idx = ctx.new_block("arrlike.elem.bounds");
    let elem_load_idx = ctx.new_block("arrlike.elem.load");
    let elem_value_idx = ctx.new_block("arrlike.elem.value");
    let object_miss_idx = ctx.new_block("arrlike.ic.miss");
    let merge_idx = ctx.new_block("arrlike.ic.merge");
    let object_header_label = ctx.block_label(object_header_idx);
    let object_brand_label = ctx.block_label(object_brand_idx);
    let object_array_guard_label = ctx.block_label(object_array_guard_idx);
    let object_array_load_label = ctx.block_label(object_array_load_idx);
    let ta_brand_label = ctx.block_label(ta_brand_idx);
    let ta_kind_guard_label = ctx.block_label(ta_kind_guard_idx);
    let ta_width_label = ctx.block_label(ta_width_idx);
    let ta_width4_label = ctx.block_label(ta_width4_idx);
    let ta_width2_label = ctx.block_label(ta_width2_idx);
    let ta_w8_label = ctx.block_label(ta_w8_idx);
    let ta_w4_label = ctx.block_label(ta_w4_idx);
    let ta_w2_label = ctx.block_label(ta_w2_idx);
    let ta_w1_label = ctx.block_label(ta_w1_idx);
    let elem_kind_label = ctx.block_label(elem_kind_idx);
    let elem_meta_label = ctx.block_label(elem_meta_idx);
    let elem_store_label = ctx.block_label(elem_store_idx);
    let elem_bounds_label = ctx.block_label(elem_bounds_idx);
    let elem_load_label = ctx.block_label(elem_load_idx);
    let elem_value_label = ctx.block_label(elem_value_idx);
    let object_miss_label = ctx.block_label(object_miss_idx);
    let merge_label = ctx.block_label(merge_idx);

    // Reject every non-pointer / handle-band / noncanonical-index case before
    // touching a managed header. The slow exit retains full ToPropertyKey,
    // typed-array, Proxy, string, descriptor, hole and prototype-chain
    // semantics.
    let heap_floor =
        crate::target_layout::heap_addr_lower_bound_inclusive(ctx.target_triple).to_string();
    let heap_ceiling =
        crate::target_layout::heap_addr_upper_bound_exclusive(ctx.target_triple).to_string();
    let (object_raw, object_entry_ok) = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(obj_box);
        let raw = blk.and(I64, &bits, pointer_mask);
        let tag = blk.and(I64, &bits, &tag_mask);
        let is_ptr = blk.icmp_eq(I64, &tag, pointer_tag);
        let above_floor = blk.icmp_uge(I64, &raw, &heap_floor);
        let below_ceiling = blk.icmp_ult(I64, &raw, &heap_ceiling);
        let idx_ge0 = blk.fcmp("oge", idx_d, "0.0");
        let idx_lt = blk.fcmp("olt", idx_d, "4294967295.0");
        let valid_ptr = blk.and(I1, &is_ptr, &above_floor);
        let valid_ptr = blk.and(I1, &valid_ptr, &below_ceiling);
        let valid_idx = blk.and(I1, &idx_ge0, &idx_lt);
        (raw, blk.and(I1, &valid_ptr, &valid_idx))
    };
    ctx.block()
        .cond_br(&object_entry_ok, &object_header_label, &object_miss_label);

    // One validated managed header feeds two tiers: a direct ordinary-Array
    // load, the typed-array arm and the elements-backed Array-subclass probe.
    ctx.current_block = object_header_idx;
    let object_idx_i64 = ctx.block().fptosi(DOUBLE, idx_d, I64);
    let object_idx_back = ctx.block().sitofp(I64, &object_idx_i64, DOUBLE);
    let object_idx_is_int = ctx.block().fcmp("oeq", &object_idx_back, idx_d);
    let gc_type_addr = ctx.block().sub(I64, &object_raw, "8");
    let gc_type_ptr = ctx.block().inttoptr(I64, &gc_type_addr);
    let gc_type = ctx.block().load(I8, &gc_type_ptr);
    let is_array = ctx.block().icmp_eq(I8, &gc_type, "1");
    let gc_flags_addr = ctx.block().sub(I64, &object_raw, "7");
    let gc_flags_ptr = ctx.block().inttoptr(I64, &gc_flags_addr);
    let gc_flags = ctx.block().load(I8, &gc_flags_ptr);
    let forwarded = ctx.block().and(I8, &gc_flags, "128");
    let not_forwarded = ctx.block().icmp_eq(I8, &forwarded, "0");
    let header_ok = ctx.block().and(I1, &object_idx_is_int, &not_forwarded);
    ctx.block()
        .cond_br(&header_ok, &object_brand_label, &object_miss_label);

    // `GC_TYPE_ARRAY` takes the direct guarded load. Everything else is offered
    // to the typed-array arm, then to the elements-backed Array-subclass
    // probe; `GC_TYPE_LAZY_ARRAY`, native Buffers and every exotic cell fail
    // both brand tests and are classified by the slow exit. Both `tav.brand`
    // and `arrlike.elem.kind` re-test the brand they need before they read a
    // header word, so nothing else can reach those loads.
    ctx.current_block = object_brand_idx;
    ctx.block()
        .cond_br(&is_array, &object_array_guard_label, &ta_brand_label);

    // Ordinary Array: the receiver tag and forwarding state were checked in
    // the predecessor.  Reject descriptors or any process-wide prototype
    // invalidation, then prove a dense in-capacity index before loading the
    // raw JSValue.  A hole is exposed as `undefined`, exactly like the guarded
    // statically-Array tier.  Every exotic/OOB case retains the unchanged
    // boxed dispatcher.
    ctx.current_block = object_array_guard_idx;
    let array_reserved_addr = ctx.block().sub(I64, &object_raw, "6");
    let array_reserved_ptr = ctx.block().inttoptr(I64, &array_reserved_addr);
    let array_reserved = ctx.block().load(I16, &array_reserved_ptr);
    let array_descriptor_bits = ctx.block().and(I16, &array_reserved, "1024");
    let array_no_descriptors = ctx.block().icmp_eq(I16, &array_descriptor_bits, "0");
    let array_invalidated = ctx
        .block()
        .load_volatile(I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
    let array_default_prototypes = ctx.block().icmp_eq(I8, &array_invalidated, "0");
    let array_ptr = ctx.block().inttoptr(I64, &object_raw);
    let array_length = ctx.block().load(I32, &array_ptr);
    let array_capacity_addr = ctx.block().add(I64, &object_raw, "4");
    let array_capacity_ptr = ctx.block().inttoptr(I64, &array_capacity_addr);
    let array_capacity = ctx.block().load(I32, &array_capacity_ptr);
    let array_length_i64 = ctx.block().zext(I32, &array_length, I64);
    let array_capacity_i64 = ctx.block().zext(I32, &array_capacity, I64);
    let array_index_in_bounds = ctx
        .block()
        .icmp_ult(I64, &object_idx_i64, &array_length_i64);
    let array_length_within_capacity =
        ctx.block()
            .icmp_ule(I64, &array_length_i64, &array_capacity_i64);
    let array_guard_ok = ctx
        .block()
        .and(I1, &array_no_descriptors, &array_default_prototypes);
    let array_guard_ok = ctx.block().and(I1, &array_guard_ok, &array_index_in_bounds);
    let array_guard_ok = ctx
        .block()
        .and(I1, &array_guard_ok, &array_length_within_capacity);
    ctx.block().cond_br(
        &array_guard_ok,
        &object_array_load_label,
        &object_miss_label,
    );

    ctx.current_block = object_array_load_idx;
    let array_base = ctx.block().array_elements_addr(&object_raw);
    let array_base_ptr = ctx.block().inttoptr(I64, &array_base);
    let array_element_ptr =
        ctx.block()
            .gep_inbounds(I64, &array_base_ptr, &[(I64, &object_idx_i64)]);
    let array_raw = ctx.block().load(DOUBLE, &array_element_ptr);
    let array_raw_bits = ctx.block().bitcast_double_to_i64(&array_raw);
    let array_is_hole = ctx
        .block()
        .icmp_eq(I64, &array_raw_bits, crate::nanbox::TAG_HOLE_I64);
    let array_undefined = ctx
        .block()
        .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
    let array_value = ctx
        .block()
        .select(I1, &array_is_hole, DOUBLE, &array_undefined, &array_raw);
    let array_value = if coerce_slow_to_number {
        ctx.block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &array_value)])
    } else {
        array_value
    };
    let array_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // ---- typed-array arm: ONE guarded read, four element widths ----
    //
    // #5525's ladder was eight per-kind load blocks behind a seven-block kind
    // dispatch, and measuring it out of line (this exit + a Rust fast path)
    // cost a dynamically-typed `Float64Array` sum **+122.9% walltime and
    // +206.3% instructions** — a call and its statepoint per element. It is
    // back inline, collapsed onto the ELEMENT WIDTH the header already stores
    // (`TypedArrayHeader::elem_size`, byte 9): four load blocks instead of
    // fifteen, each of which resolves its own signedness/float form with
    // `select`s rather than more branches.
    //
    // (The literal "cache the last-seen kind, inline one guarded load" form is
    // not expressible in static codegen: the LOAD TYPE is what varies per
    // kind, so a runtime-cached kind still cannot pick it. Width is the
    // coarsest split that keeps every load in bounds — reading 8 bytes from a
    // `Uint8Array`'s last element is not ours to take.)
    //
    // `PERRY_TA_VIEW_GUARD == 0` is the licence to compute the data pointer as
    // `header + 16` without consulting the view registries; a raised guard,
    // like a BigInt lane or an out-of-range index, leaves through
    // `arrlike.ic.miss` — the same exit the old `tav.get.slow` edge reached.
    //
    // This arm sits AHEAD of the object probe, which was measured both ways:
    // putting it on the probe's decline edge instead saves an object receiver
    // one brand test it always fails, and that was worth nothing on any
    // workload (the row it was meant to fix, `bench_histogram_numarray`, read
    // +0.15% retired instructions either way and +0.00% once rebuilt), while
    // costing every typed-array element read an `icmp`+branch — `dyn_ta_f64`
    // went from -9.98% to -6.65%.
    //
    // #10118: the brand test DECIDES ON THE TAG ALONE. Everything else in the
    // guard set — the view guard, the element kind and its range test, the
    // bounds check — is meaningful only once the tag says typed array, so it
    // sits behind the tag in `tav.kind_guard`. An Array-subclass instance or a
    // `JSON.parse` array reaching this arm pays one `icmp` and leaves, instead
    // of three loads and three ANDs to reach a branch it was always going to
    // take. The typed-array path reaches the same guard set by the same
    // AND-reduction and is unchanged.
    ctx.current_block = ta_brand_idx;
    let is_typed_array = ctx.block().icmp_eq(I8, &gc_type, "11"); // GC_TYPE_TYPED_ARRAY
    ctx.block()
        .cond_br(&is_typed_array, &ta_kind_guard_label, &elem_kind_label);

    // Past the tag, a receiver this guard rejects (a raised view guard, a
    // BigInt/Float16 lane, an out-of-range index) cannot be an ordinary object
    // either, so it leaves straight through the exit rather than re-testing
    // `GC_TYPE_OBJECT` it is guaranteed to fail.
    ctx.current_block = ta_kind_guard_idx;
    let (ta_kind, ta_ok) = {
        let blk = ctx.block();
        let view_guard = blk.load(I64, "@PERRY_TA_VIEW_GUARD");
        let inline_storage = blk.icmp_eq(I64, &view_guard, "0");
        let kind_addr = blk.add(I64, &object_raw, "8");
        let kind_ptr = blk.inttoptr(I64, &kind_addr);
        let kind_i8 = blk.load(I8, &kind_ptr);
        let kind = blk.zext(I8, &kind_i8, I64);
        // kinds 0..=8 (Int8 .. Uint8Clamped); rejects BigInt 9/10 and Float16
        // 11, whose lanes are not plain Numbers.
        let kind_ok = blk.icmp_ule(I64, &kind, "8");
        // `length` is `TypedArrayHeader` word 0.
        let len_ptr = blk.inttoptr(I64, &object_raw);
        let len = blk.load(I32, &len_ptr);
        let len_i64 = blk.zext(I32, &len, I64);
        let in_bounds = blk.icmp_ult(I64, &object_idx_i64, &len_i64);
        let ok = blk.and(I1, &inline_storage, &kind_ok);
        (kind, blk.and(I1, &ok, &in_bounds))
    };
    ctx.block()
        .cond_br(&ta_ok, &ta_width_label, &object_miss_label);

    // `elem_size` (byte 9) is written from `kind` by `typed_array_alloc`, so
    // the brand guard's `kind <= KIND_UINT8_CLAMPED` already bounds it to
    // {1,2,4,8} — the same pairing the runtime's own `load_at` trusts for its
    // offset and its load type. `tav.w1` is the final else, not a fourth test.
    ctx.current_block = ta_width_idx;
    let (ta_elem_size, ta_addr) = {
        let blk = ctx.block();
        let size_addr = blk.add(I64, &object_raw, "9");
        let size_ptr = blk.inttoptr(I64, &size_addr);
        let size_i8 = blk.load(I8, &size_ptr);
        let elem_size = blk.zext(I8, &size_i8, I64);
        let offset = blk.mul(I64, &object_idx_i64, &elem_size);
        // `data = header + size_of::<TypedArrayHeader>()`, proven by the
        // cleared view guard above.
        let data_base = blk.add(I64, &object_raw, "16");
        (elem_size, blk.add(I64, &data_base, &offset))
    };
    let is_width8 = ctx.block().icmp_eq(I64, &ta_elem_size, "8");
    ctx.block()
        .cond_br(&is_width8, &ta_w8_label, &ta_width4_label);

    ctx.current_block = ta_width4_idx;
    let is_width4 = ctx.block().icmp_eq(I64, &ta_elem_size, "4");
    ctx.block()
        .cond_br(&is_width4, &ta_w4_label, &ta_width2_label);

    ctx.current_block = ta_width2_idx;
    let is_width2 = ctx.block().icmp_eq(I64, &ta_elem_size, "2");
    ctx.block().cond_br(&is_width2, &ta_w2_label, &ta_w1_label);

    // Width 8: `Float64Array` is the only non-BigInt kind of this width, so
    // the stored lane IS the value — and therefore an arbitrary 64-bit pattern
    // the program wrote through some other view. #10779: canonicalise its NaNs
    // before the value leaves as a JS value.
    ctx.current_block = ta_w8_idx;
    let ta_w8_value = {
        let blk = ctx.block();
        let ptr = blk.inttoptr(I64, &ta_addr);
        let lane = blk.load(DOUBLE, &ptr);
        crate::expr::nanbox_inline::canonicalize_lane_f64(blk, &lane)
    };
    let ta_w8_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // Width 4: `Int32Array` (4), `Uint32Array` (5), `Float32Array` (6). Both
    // integer forms come from ONE load: the zero-extended lane is the unsigned
    // value, `shl`+`ashr` is the signed one, and `sitofp i64` is exact for
    // both because a u32 fits a signed i64. The float form reinterprets the
    // same lane; neither computation can trap, so a `select` replaces the
    // branch.
    ctx.current_block = ta_w4_idx;
    let ta_w4_value = {
        let blk = ctx.block();
        let ptr = blk.inttoptr(I64, &ta_addr);
        let lane = blk.load(I32, &ptr);
        let unsigned = blk.zext(I32, &lane, I64);
        let widened = blk.shl(I64, &unsigned, "32");
        let signed = blk.ashr(I64, &widened, "32");
        let is_signed = blk.icmp_eq(I64, &ta_kind, "4");
        let integral = blk.select(I1, &is_signed, I64, &signed, &unsigned);
        let as_number = blk.sitofp(I64, &integral, DOUBLE);
        let as_f32 = blk.bitcast_i32_to_float(&lane);
        let widened_f32 = blk.fpext(F32, &as_f32, DOUBLE);
        // #10779: an f32 NaN widens to an f64 NaN that KEEPS its payload —
        // `0x7FFFFFFF` becomes `0x7FFF_FFFF_E000_0000`, a forged string
        // pointer. The integer arms of this select cannot be NaN, so
        // canonicalising the f32 arm alone is enough.
        let widened_f32 = crate::expr::nanbox_inline::canonicalize_lane_f64(blk, &widened_f32);
        let is_f32 = blk.icmp_eq(I64, &ta_kind, "6");
        blk.select(I1, &is_f32, DOUBLE, &widened_f32, &as_number)
    };
    let ta_w4_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // Width 2: `Int16Array` (2) and `Uint16Array` (3).
    ctx.current_block = ta_w2_idx;
    let ta_w2_value = {
        let blk = ctx.block();
        let ptr = blk.inttoptr(I64, &ta_addr);
        let lane = blk.load(I16, &ptr);
        let unsigned = blk.zext(I16, &lane, I64);
        let widened = blk.shl(I64, &unsigned, "48");
        let signed = blk.ashr(I64, &widened, "48");
        let is_signed = blk.icmp_eq(I64, &ta_kind, "2");
        let integral = blk.select(I1, &is_signed, I64, &signed, &unsigned);
        blk.sitofp(I64, &integral, DOUBLE)
    };
    let ta_w2_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // Width 1: `Int8Array` (0), `Uint8Array` (1) and `Uint8ClampedArray` (8) —
    // the clamped kind stores plain bytes, so it shares the unsigned form.
    ctx.current_block = ta_w1_idx;
    let ta_w1_value = {
        let blk = ctx.block();
        let ptr = blk.inttoptr(I64, &ta_addr);
        let lane = blk.load(I8, &ptr);
        let unsigned = blk.zext(I8, &lane, I64);
        let widened = blk.shl(I64, &unsigned, "56");
        let signed = blk.ashr(I64, &widened, "56");
        let is_signed = blk.icmp_eq(I64, &ta_kind, "0");
        let integral = blk.select(I1, &is_signed, I64, &signed, &unsigned);
        blk.sitofp(I64, &integral, DOUBLE)
    };
    let ta_w1_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // ---- elements-backed Array-subclass probe ----
    //
    // `class X extends Array` instances own a real `GC_TYPE_ARRAY` in
    // `ObjectMeta.elements` (`perry-runtime/src/array/subclass_elements.rs`),
    // so the read is the plain Array read on that inner array: meta word ->
    // `elements` (word 12) -> bounds -> slot. A miss (no meta, no store) and
    // every hole or out-of-range index go to the exit, which keeps the
    // complete prototype-chain semantics.
    //
    // This tier is INLINE because the elements store is the DEFAULT
    // representation. Moving it out of line cost an `Array`-subclass read loop
    // **+71.2% instructions and +60.0% walltime** (`dyn_arraylike_object`).
    //
    // What is NOT here any more is the shape-carried IC tower —
    // `arrlike.ic.{shape,identity,exact,family_meta,family_token,bounds,
    // length_inline,length_spill_meta,length_spill_ptr,length_spill_load,
    // range,inline,spill,spill_ptr,spill_load,spill_or_miss}`, fifteen blocks
    // at every site. Its hit requires a primed layout cache, and the only
    // writer of that cache (`js_packed_arraylike_index_get` ->
    // `dense_layout_for_validated_object` -> `build_dense_layout`) is reached
    // only when `elements_of(obj)` is NULL and the receiver passes
    // `is_array_subclass_class_id`. With the elements store on — the shipped
    // default, whose kill switch `PERRY_ARRAY_SUBCLASS_ELEMENTS=0` its own
    // doc calls "a bisecting kill switch, not a supported mode" — that cache
    // is never primed, so `cached_key` is always zero and the whole tower
    // exits on its first test. It was fifteen unreachable blocks per site,
    // inlined at 10,778 sites in `prettier/plugins/flow.mjs` alone. Under the
    // kill switch those receivers now take one call per read instead.
    ctx.current_block = elem_kind_idx;
    let elem_is_object = ctx.block().icmp_eq(I8, &gc_type, "2");
    ctx.block()
        .cond_br(&elem_is_object, &elem_meta_label, &object_miss_label);

    ctx.current_block = elem_meta_idx;
    let meta_ptr_size: u64 = if crate::target_layout::target_is_ilp32(ctx.target_triple) {
        4
    } else {
        8
    };
    let meta_offset =
        crate::target_layout::object_meta_slot_offset_bytes(ctx.target_triple).to_string();
    let elem_meta_addr = ctx.block().add(I64, &object_raw, &meta_offset);
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
        .cond_br(&elem_has_meta, &elem_store_label, &object_miss_label);

    ctx.current_block = elem_store_idx;
    let elem_meta_ptr = ctx.block().inttoptr(I64, &elem_meta_i64);
    // `ObjectMeta.elements` is word 12 (offset 96; pinned by a const assert
    // in perry-runtime `object/mod.rs`).
    let elem_store_slot_ptr = ctx.block().gep(I64, &elem_meta_ptr, &[(I64, "12")]);
    let elem_store_i64 = ctx.block().load(I64, &elem_store_slot_ptr);
    let elem_has_store = ctx.block().icmp_ne(I64, &elem_store_i64, "0");
    ctx.block()
        .cond_br(&elem_has_store, &elem_bounds_label, &object_miss_label);

    ctx.current_block = elem_bounds_idx;
    let elem_type_addr = ctx.block().sub(I64, &elem_store_i64, "8");
    let elem_type_ptr = ctx.block().inttoptr(I64, &elem_type_addr);
    let elem_type = ctx.block().load(I8, &elem_type_ptr);
    let elem_is_array = ctx.block().icmp_eq(I8, &elem_type, "1");
    let elem_flags_addr = ctx.block().sub(I64, &elem_store_i64, "7");
    let elem_flags_ptr = ctx.block().inttoptr(I64, &elem_flags_addr);
    let elem_flags = ctx.block().load(I8, &elem_flags_ptr);
    let elem_fwd = ctx.block().and(I8, &elem_flags, "128");
    let elem_not_fwd = ctx.block().icmp_eq(I8, &elem_fwd, "0");
    let elem_store_ptr = ctx.block().inttoptr(I64, &elem_store_i64);
    let elem_length = ctx.block().load(I32, &elem_store_ptr);
    let elem_length_i64 = ctx.block().zext(I32, &elem_length, I64);
    let elem_in_bounds = ctx.block().icmp_ult(I64, &object_idx_i64, &elem_length_i64);
    let elem_ok = ctx.block().and(I1, &elem_is_array, &elem_not_fwd);
    let elem_ok = ctx.block().and(I1, &elem_ok, &elem_in_bounds);
    ctx.block()
        .cond_br(&elem_ok, &elem_load_label, &object_miss_label);

    ctx.current_block = elem_load_idx;
    let elem_bytes = ctx.block().shl(I64, &object_idx_i64, "3");
    let elem_elements_addr = ctx.block().array_elements_addr(&elem_store_i64);
    let elem_addr = ctx.block().add(I64, &elem_elements_addr, &elem_bytes);
    let elem_ptr = ctx.block().inttoptr(I64, &elem_addr);
    let elem_raw = ctx.block().load(DOUBLE, &elem_ptr);
    let elem_bits = ctx.block().bitcast_double_to_i64(&elem_raw);
    let elem_is_hole = ctx
        .block()
        .icmp_eq(I64, &elem_bits, crate::nanbox::TAG_HOLE_I64);
    ctx.block()
        .cond_br(&elem_is_hole, &object_miss_label, &elem_value_label);

    ctx.current_block = elem_value_idx;
    let elem_value = if coerce_slow_to_number {
        ctx.block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &elem_raw)])
    } else {
        elem_raw
    };
    let elem_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // The site's ONE out-of-line edge, and it is the SAME call the old
    // `arrlike.ic.miss` block made: every arm this change stopped inlining was
    // an acceleration of a decision `js_packed_arraylike_index_get` already
    // makes, and it is handed this site's own cache slot, so neither the
    // answer nor the primed words moved.
    //
    // Deliberately NOT a new four-argument entry point with the cold
    // `ToNumber` folded into a flag: the extra argument and its test cost
    // +0.43% retired instructions on `object_deep_clone` and +0.18% on
    // `json_parse_1mb`, paid by every receiver that reaches this exit, to
    // spare a `js_number_coerce` from an arm that no TypeScript fixture can
    // reach (see `lower_unknown_local_index_get_for_number_context`).
    ctx.current_block = object_miss_idx;
    let slow_raw = ctx.block().call(
        DOUBLE,
        "js_packed_arraylike_index_get",
        &[(DOUBLE, obj_box), (DOUBLE, idx_d), (PTR, &slot_ref)],
    );
    // In a number context, coerce the (possibly boxed) slow result here so the
    // merge phi is uniformly a Number and the arithmetic caller skips its own
    // per-element coerce. A plain double already shortcuts `js_number_coerce`'s
    // first branch, so re-coercing a fast-path-shaped value is a cheap no-op on
    // the rare cache-miss path.
    let slow_val = if coerce_slow_to_number {
        ctx.block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &slow_raw)])
    } else {
        slow_raw
    };
    let slow_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // ---- final merge: the two inline hits and the single exit ----
    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (ta_w8_value.as_str(), ta_w8_end.as_str()),
            (ta_w4_value.as_str(), ta_w4_end.as_str()),
            (ta_w2_value.as_str(), ta_w2_end.as_str()),
            (ta_w1_value.as_str(), ta_w1_end.as_str()),
            (array_value.as_str(), array_end_label.as_str()),
            (elem_value.as_str(), elem_end_label.as_str()),
            (slow_val.as_str(), slow_end_label.as_str()),
        ],
    )
}
