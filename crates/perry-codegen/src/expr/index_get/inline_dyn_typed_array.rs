//! Inline guarded typed-array element read for a dynamically-typed receiver.
//!
//! Split out of `index_get.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — the items below are verbatim copies (only the
//! visibility of the entry point is widened to `pub(super)` so the trunk's
//! call sites keep compiling).
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

/// #5525 follow-up: emit a guarded **inline** typed-array element read for an
/// `obj[i]` whose receiver static type is erased (`any`/unknown) but is, at
/// runtime, commonly an owning numeric typed array reached through an untyped
/// param — exactly bcryptjs's `S[i]`/`P[i]` Blowfish boxes (~600M reads for one
/// cost-12 `compareSync`). Instead of an out-of-line `js_dyn_index_get` call +
/// `lookup_typed_array_kind` + `js_number_coerce` per element, this inlines:
///   1. receiver-is-pointer NaN-box guard,
///   2. a read of the process-global `PERRY_TA_VIEW_GUARD` (must be 0 → every
///      live typed array uses inline storage, so `data_ptr == header + 16`),
///   3. a probe of the `PERRY_TA_KIND_CACHE` slot for the receiver address
///      (matches the cached `(addr << 8) | tag` word; the tag is the element
///      kind and must be a non-BigInt kind ≤ `KIND_UINT8_CLAMPED`),
///   4. an index validity + bounds check against the header `length`,
///   5. a direct per-kind element load + int↔f64 widen,
/// and falls back to the existing `js_dyn_index_get` slow path on ANY guard
/// miss (non-pointer, cache miss, view live, BigInt/Float16 kind, OOB /
/// fractional / negative index, runtime-string or symbol key). Because every
/// rejected case defers to the unchanged runtime helper, semantics are
/// identical; only the hot monomorphic numeric-typed-array case is short-cut.
/// `obj_box` / `idx_d` are the already-lowered receiver and index (DOUBLE).
///
/// `coerce_slow_to_number`: when the read is used in a context that will
/// `ToNumber` the result regardless (a non-`+` arithmetic / bitwise operand —
/// `^`, `-`, `*`, `<<`, …, all of which `ToNumber` their operands; see
/// [`lower_unknown_local_index_get_for_number_context`]), the cold slow branch's
/// `js_dyn_index_get` result is wrapped in `js_number_coerce` here so the merged
/// value is *always* a Number. The hot per-kind fast branches already produce a
/// Number, so the caller can skip the per-element site `js_number_coerce` it
/// would otherwise emit — moving that coercion off bcrypt's ~600M-read hot path
/// and onto the cache-miss path only. `false` leaves the slow result boxed
/// (the general `obj[i]` read, whose result may legitimately be a non-Number).
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

    let fast_idx = ctx.new_block("tav.get.fast");
    let load_idx = ctx.new_block("tav.get.load");
    let slow_idx = ctx.new_block("tav.get.slow");
    let merge_idx = ctx.new_block("tav.get.merge");
    let fast_label = ctx.block_label(fast_idx);
    let load_label = ctx.block_label(load_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    // ---- entry: combined cache/kind/range guard -> fast | slow ----
    let entry_guard = {
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(obj_box);
        let raw = blk.and(I64, &obj_bits, pointer_mask);
        // is_pointer: (bits & TAG_MASK) == POINTER_TAG
        let tagged = blk.and(I64, &obj_bits, &tag_mask);
        let is_ptr = blk.icmp_eq(I64, &tagged, pointer_tag);
        // view guard must be 0 (all typed arrays inline-storage)
        let vg = blk.load(I64, "@PERRY_TA_VIEW_GUARD");
        let vg_zero = blk.icmp_eq(I64, &vg, "0");
        // cache slot = (raw >> 3) & 63
        let slot = blk.lshr(I64, &raw, "3");
        let slot = blk.and(I64, &slot, "63");
        let entry_ptr = blk.gep(
            "[64 x i64]",
            "@PERRY_TA_KIND_CACHE",
            &[(I64, "0"), (I64, &slot)],
        );
        let entry_val = blk.load(I64, &entry_ptr);
        // addr match: (entry_val u>> 8) == raw  (also rejects empty slot = 0)
        let entry_addr = blk.lshr(I64, &entry_val, "8");
        let addr_match = blk.icmp_eq(I64, &entry_addr, &raw);
        // kind = entry_val & 0xFF; loadable numeric kind = kind <= 8
        // (KIND_INT8=0 .. KIND_UINT8_CLAMPED=8; rejects BigInt 9/10,
        // Float16 11, and the 0xFF "not a typed array" sentinel).
        let kind = blk.and(I64, &entry_val, "255");
        let kind_ok = blk.icmp_ule(I64, &kind, "8");
        // index float-range pre-checks (well-defined on NaN → false): the
        // fptosi in the load block is only reached when these hold, so its
        // result is never poison there.
        let idx_ge0 = blk.fcmp("oge", idx_d, "0.0");
        let idx_lt = blk.fcmp("olt", idx_d, "4294967296.0");
        // AND-reduce all guards.
        let g = blk.and(I1, &is_ptr, &vg_zero);
        let g = blk.and(I1, &g, &addr_match);
        let g = blk.and(I1, &g, &kind_ok);
        let g = blk.and(I1, &g, &idx_ge0);
        blk.and(I1, &g, &idx_lt)
    };
    ctx.block().cond_br(&entry_guard, &fast_label, &slow_label);

    // ---- fast: validate integer index + bounds -> load | slow ----
    ctx.current_block = fast_idx;
    let (raw, idx_i64, kind) = {
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(obj_box);
        let raw = blk.and(I64, &obj_bits, pointer_mask);
        // kind re-read from cache (cheap; keeps the fast block self-contained).
        let slot = blk.lshr(I64, &raw, "3");
        let slot = blk.and(I64, &slot, "63");
        let entry_ptr = blk.gep(
            "[64 x i64]",
            "@PERRY_TA_KIND_CACHE",
            &[(I64, "0"), (I64, &slot)],
        );
        let entry_val = blk.load(I64, &entry_ptr);
        let kind = blk.and(I64, &entry_val, "255");
        // idx is in [0, 2^32) (entry guard) so fptosi i64 is well-defined.
        let idx_i64 = blk.fptosi(DOUBLE, idx_d, I64);
        (raw, idx_i64, kind)
    };
    let fast_ok = {
        let blk = ctx.block();
        // reject fractional indices: sitofp(idx_i64) == idx_d
        let idx_back = blk.sitofp(I64, &idx_i64, DOUBLE);
        let is_int = blk.fcmp("oeq", &idx_back, idx_d);
        // bounds: idx < header.length (u32 at offset 0)
        let hdr_ptr = blk.inttoptr(I64, &raw);
        let len = blk.load(I32, &hdr_ptr);
        let len_i64 = blk.zext(I32, &len, I64);
        let in_bounds = blk.icmp_ult(I64, &idx_i64, &len_i64);
        blk.and(I1, &is_int, &in_bounds)
    };
    ctx.block().cond_br(&fast_ok, &load_label, &slow_label);

    // ---- load: per-kind direct element load (data = header + 16) ----
    ctx.current_block = load_idx;
    // (value, end_label) for each per-kind load block, collected for the merge.
    let mut kind_incoming: Vec<(String, String)>;
    {
        // Per-kind load blocks. Each computes the element address from
        // `data = raw + 16` and `off = idx * elem_size`, loads the native
        // slot, and widens to f64. We branch on `kind` via a cond_br chain.
        // kinds: 0 I8, 1 U8, 2 I16, 3 U16, 4 I32, 5 U32, 6 F32, 7 F64,
        // 8 U8Clamped (== U8 load). All others were excluded by the entry
        // guard (kind <= 8).
        let data_base = {
            let blk = ctx.block();
            blk.add(I64, &raw, "16")
        };
        // Helper closure-like inline: build a block that loads with a given
        // element byte-width shift + LLVM elem type + widen, then brs to merge.
        // We emit explicit blocks since closures can't borrow ctx mutably here.

        // Create the per-kind blocks up front.
        let b_i8 = ctx.new_block("tav.k.i8");
        let b_u8 = ctx.new_block("tav.k.u8");
        let b_i16 = ctx.new_block("tav.k.i16");
        let b_u16 = ctx.new_block("tav.k.u16");
        let b_i32 = ctx.new_block("tav.k.i32");
        let b_u32 = ctx.new_block("tav.k.u32");
        let b_f32 = ctx.new_block("tav.k.f32");
        let b_f64 = ctx.new_block("tav.k.f64");
        let l_i8 = ctx.block_label(b_i8);
        let l_u8 = ctx.block_label(b_u8);
        let l_i16 = ctx.block_label(b_i16);
        let l_u16 = ctx.block_label(b_u16);
        let l_i32 = ctx.block_label(b_i32);
        let l_u32 = ctx.block_label(b_u32);
        let l_f32 = ctx.block_label(b_f32);
        let l_f64 = ctx.block_label(b_f64);

        // Dispatch chain on `kind` (in the load block).
        let chk = |ctx: &mut FnCtx<'_>, k: &str, hit: &str, next_idx: usize| {
            let next_label = ctx.block_label(next_idx);
            let cond = ctx.block().icmp_eq(I64, &kind, k);
            ctx.block().cond_br(&cond, hit, &next_label);
        };
        // 0..7 explicit; kind 8 (U8Clamped) shares the U8 load as the final
        // else (no further branch needed — entry guard already proved kind<=8).
        let c1 = ctx.new_block("tav.kd1");
        let c2 = ctx.new_block("tav.kd2");
        let c3 = ctx.new_block("tav.kd3");
        let c4 = ctx.new_block("tav.kd4");
        let c5 = ctx.new_block("tav.kd5");
        let c6 = ctx.new_block("tav.kd6");
        let c7 = ctx.new_block("tav.kd7");
        chk(ctx, "0", &l_i8, c1);
        ctx.current_block = c1;
        chk(ctx, "1", &l_u8, c2);
        ctx.current_block = c2;
        chk(ctx, "2", &l_i16, c3);
        ctx.current_block = c3;
        chk(ctx, "3", &l_u16, c4);
        ctx.current_block = c4;
        chk(ctx, "4", &l_i32, c5);
        ctx.current_block = c5;
        chk(ctx, "5", &l_u32, c6);
        ctx.current_block = c6;
        chk(ctx, "6", &l_f32, c7);
        ctx.current_block = c7;
        // remaining: kind 7 → f64, else (8) → u8.
        let is_f64 = ctx.block().icmp_eq(I64, &kind, "7");
        ctx.block().cond_br(&is_f64, &l_f64, &l_u8);

        // Each per-kind block: compute elem addr, load, widen, br merge.
        // off = idx << shift; addr = data_base + off.
        let mut incoming: Vec<(String, String)> = Vec::new();
        // I8 (sext), U8 (zext), I16 (sext), U16 (zext) via the small-int helper.
        incoming.push(emit_inline_ta_int_load(
            ctx,
            b_i8,
            &idx_i64,
            &data_base,
            &merge_label,
            "0",
            I8,
            true,
        ));
        incoming.push(emit_inline_ta_int_load(
            ctx,
            b_u8,
            &idx_i64,
            &data_base,
            &merge_label,
            "0",
            I8,
            false,
        ));
        incoming.push(emit_inline_ta_int_load(
            ctx,
            b_i16,
            &idx_i64,
            &data_base,
            &merge_label,
            "1",
            I16,
            true,
        ));
        incoming.push(emit_inline_ta_int_load(
            ctx,
            b_u16,
            &idx_i64,
            &data_base,
            &merge_label,
            "1",
            I16,
            false,
        ));
        // I32: load i32, sitofp directly (sext to i32 is a no-op).
        {
            ctx.current_block = b_i32;
            let blk = ctx.block();
            let off = blk.shl(I64, &idx_i64, "2");
            let addr = blk.add(I64, &data_base, &off);
            let ptr = blk.inttoptr(I64, &addr);
            let raw_elem = blk.load(I32, &ptr);
            let val = blk.sitofp(I32, &raw_elem, DOUBLE);
            let end_label = blk.label.clone();
            blk.br(&merge_label);
            incoming.push((val, end_label));
        }
        // U32: load i32, treat as unsigned → uitofp.
        {
            ctx.current_block = b_u32;
            let blk = ctx.block();
            let off = blk.shl(I64, &idx_i64, "2");
            let addr = blk.add(I64, &data_base, &off);
            let ptr = blk.inttoptr(I64, &addr);
            let raw_elem = blk.load(I32, &ptr);
            let val = blk.uitofp(I32, &raw_elem, DOUBLE);
            let end_label = blk.label.clone();
            blk.br(&merge_label);
            incoming.push((val, end_label));
        }
        // F32: load float, fpext.
        {
            ctx.current_block = b_f32;
            let blk = ctx.block();
            let off = blk.shl(I64, &idx_i64, "2");
            let addr = blk.add(I64, &data_base, &off);
            let ptr = blk.inttoptr(I64, &addr);
            let raw_elem = blk.load(F32, &ptr);
            let val = blk.fpext(F32, &raw_elem, DOUBLE);
            let end_label = blk.label.clone();
            blk.br(&merge_label);
            incoming.push((val, end_label));
        }
        // F64: load double raw.
        {
            ctx.current_block = b_f64;
            let blk = ctx.block();
            let off = blk.shl(I64, &idx_i64, "3");
            let addr = blk.add(I64, &data_base, &off);
            let ptr = blk.inttoptr(I64, &addr);
            let val = blk.load(DOUBLE, &ptr);
            let end_label = blk.label.clone();
            blk.br(&merge_label);
            incoming.push((val, end_label));
        }

        // Hand the collected per-kind (value,label) pairs to the final merge.
        kind_incoming = incoming;
    }

    // ---- typed-array miss: Array-subclass shape IC, then dispatcher ----
    ctx.current_block = slow_idx;
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache_name = super::super::inline_cache_global_name(ctx, site_id);
    ctx.ic_globals.push(cache_name.clone());
    let cache_ref = format!("@{cache_name}");

    let object_header_idx = ctx.new_block("arrlike.ic.header");
    let object_bounds_idx = ctx.new_block("arrlike.ic.bounds");
    let object_inline_idx = ctx.new_block("arrlike.ic.inline");
    let object_spill_idx = ctx.new_block("arrlike.ic.spill");
    let object_spill_ptr_idx = ctx.new_block("arrlike.ic.spill_ptr");
    let object_spill_load_idx = ctx.new_block("arrlike.ic.spill_load");
    let object_miss_idx = ctx.new_block("arrlike.ic.miss");
    let object_header_label = ctx.block_label(object_header_idx);
    let object_bounds_label = ctx.block_label(object_bounds_idx);
    let object_inline_label = ctx.block_label(object_inline_idx);
    let object_spill_label = ctx.block_label(object_spill_idx);
    let object_spill_ptr_label = ctx.block_label(object_spill_ptr_idx);
    let object_spill_load_label = ctx.block_label(object_spill_load_idx);
    let object_miss_label = ctx.block_label(object_miss_idx);

    // Reject every non-pointer / handle-band / noncanonical-index case before
    // touching a managed header. The miss helper retains full ToPropertyKey,
    // Proxy, string, descriptor, hole and prototype-chain semantics.
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

    // Exact class + semantic ShapeId identity. The runtime primes only a
    // prototype-unmodified dense Array-subclass shape with no relevant
    // accessors, and publishes no heap pointer in this cache.
    ctx.current_block = object_header_idx;
    let object_idx_i64 = ctx.block().fptosi(DOUBLE, idx_d, I64);
    let object_idx_back = ctx.block().sitofp(I64, &object_idx_i64, DOUBLE);
    let object_idx_is_int = ctx.block().fcmp("oeq", &object_idx_back, idx_d);
    let gc_type_addr = ctx.block().sub(I64, &object_raw, "8");
    let gc_type_ptr = ctx.block().inttoptr(I64, &gc_type_addr);
    let gc_type = ctx.block().load(I8, &gc_type_ptr);
    let is_object = ctx.block().icmp_eq(I8, &gc_type, "2");
    let gc_flags_addr = ctx.block().sub(I64, &object_raw, "7");
    let gc_flags_ptr = ctx.block().inttoptr(I64, &gc_flags_addr);
    let gc_flags = ctx.block().load(I8, &gc_flags_ptr);
    let forwarded = ctx.block().and(I8, &gc_flags, "1");
    let not_forwarded = ctx.block().icmp_eq(I8, &forwarded, "0");
    let object_ptr = ctx.block().inttoptr(I64, &object_raw);
    let class_id = ctx.block().load(I32, &object_ptr);
    let shape_addr = ctx.block().add(I64, &object_raw, "4");
    let shape_ptr = ctx.block().inttoptr(I64, &shape_addr);
    let shape_id = ctx.block().load(I32, &shape_ptr);
    let class64 = ctx.block().zext(I32, &class_id, I64);
    let shape64 = ctx.block().zext(I32, &shape_id, I64);
    let class_high = ctx.block().shl(I64, &class64, "32");
    let live_key = ctx.block().or(I64, &class_high, &shape64);
    let cached_key_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "0")]);
    let cached_key = ctx.block().load(I64, &cached_key_ptr);
    let key_matches = ctx.block().icmp_eq(I64, &live_key, &cached_key);
    let key_nonzero = ctx.block().icmp_ne(I64, &cached_key, "0");
    let object_ok = ctx.block().and(I1, &object_idx_is_int, &is_object);
    let object_ok = ctx.block().and(I1, &object_ok, &not_forwarded);
    let object_ok = ctx.block().and(I1, &object_ok, &key_matches);
    let object_ok = ctx.block().and(I1, &object_ok, &key_nonzero);
    ctx.block()
        .cond_br(&object_ok, &object_bounds_label, &object_miss_label);

    // The exact shape proves the cached length slot is live and inline. Check
    // its current value and the proved dense prefix on every hit; growing
    // `length` without creating properties therefore cannot expose holes.
    ctx.current_block = object_bounds_idx;
    let length_slot_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "1")]);
    let length_slot = ctx.block().load(I64, &length_slot_ptr);
    let element_base_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "2")]);
    let element_base = ctx.block().load(I64, &element_base_ptr);
    let dense_prefix_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "3")]);
    let dense_prefix = ctx.block().load(I64, &dense_prefix_ptr);
    let inline_bound_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "4")]);
    let inline_bound = ctx.block().load(I64, &inline_bound_ptr);
    let object_header_size =
        crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let length_bytes = ctx.block().shl(I64, &length_slot, "3");
    let length_offset = ctx.block().add(I64, &length_bytes, &object_header_size);
    let length_addr = ctx.block().add(I64, &object_raw, &length_offset);
    let length_ptr = ctx.block().inttoptr(I64, &length_addr);
    let live_length = ctx.block().load(DOUBLE, &length_ptr);
    let below_length = ctx.block().fcmp("olt", idx_d, &live_length);
    let below_prefix = ctx.block().icmp_ult(I64, &object_idx_i64, &dense_prefix);
    let in_dense_range = ctx.block().and(I1, &below_length, &below_prefix);
    let object_slot = ctx.block().add(I64, &element_base, &object_idx_i64);
    let slot_is_inline = ctx.block().icmp_ult(I64, &object_slot, &inline_bound);
    let inline_ok = ctx.block().and(I1, &in_dense_range, &slot_is_inline);
    let slot_is_spilled = ctx.block().xor(I1, &slot_is_inline, "true");
    let range_but_spilled = ctx.block().and(I1, &in_dense_range, &slot_is_spilled);
    let spill_or_miss_idx = ctx.new_block("arrlike.ic.spill_or_miss");
    let spill_or_miss_label = ctx.block_label(spill_or_miss_idx);
    ctx.block()
        .cond_br(&inline_ok, &object_inline_label, &spill_or_miss_label);
    ctx.current_block = spill_or_miss_idx;
    ctx.block()
        .cond_br(&range_but_spilled, &object_spill_label, &object_miss_label);

    ctx.current_block = object_inline_idx;
    let inline_bytes = ctx.block().shl(I64, &object_slot, "3");
    let inline_offset = ctx.block().add(I64, &inline_bytes, &object_header_size);
    let inline_addr = ctx.block().add(I64, &object_raw, &inline_offset);
    let inline_ptr = ctx.block().inttoptr(I64, &inline_addr);
    let inline_raw = ctx.block().load(DOUBLE, &inline_ptr);
    let inline_value = if coerce_slow_to_number {
        ctx.block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &inline_raw)])
    } else {
        inline_raw
    };
    let inline_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // Wide subclass instances store absolute field slots in the object-owned
    // spill Array. Reload both moving pointers from the live receiver; the IC
    // itself contains only scalar offsets.
    ctx.current_block = object_spill_idx;
    let meta_ptr_size: u64 = if crate::target_layout::target_is_ilp32(ctx.target_triple) {
        4
    } else {
        8
    };
    let meta_offset = (crate::target_layout::object_header_size_bytes(ctx.target_triple)
        - meta_ptr_size)
        .to_string();
    let meta_addr = ctx.block().add(I64, &object_raw, &meta_offset);
    let meta_slot_ptr = ctx.block().inttoptr(I64, &meta_addr);
    let meta_loaded = ctx
        .block()
        .load(if meta_ptr_size == 4 { I32 } else { I64 }, &meta_slot_ptr);
    let meta_i64 = if meta_ptr_size == 4 {
        ctx.block().zext(I32, &meta_loaded, I64)
    } else {
        meta_loaded
    };
    let has_meta = ctx.block().icmp_ne(I64, &meta_i64, "0");
    ctx.block()
        .cond_br(&has_meta, &object_spill_ptr_label, &object_miss_label);

    ctx.current_block = object_spill_ptr_idx;
    let meta_ptr = ctx.block().inttoptr(I64, &meta_i64);
    let spill_slot_ptr = ctx.block().gep(I64, &meta_ptr, &[(I64, "4")]);
    let spill_i64 = ctx.block().load(I64, &spill_slot_ptr);
    let has_spill = ctx.block().icmp_ne(I64, &spill_i64, "0");
    // Keep the hot path to one bounds branch without speculatively loading
    // through a null spill pointer: ObjectMeta is live here and is a safe
    // address for the ignored length load when `spill_i64 == 0`.
    let safe_spill_i64 = ctx
        .block()
        .select(I1, &has_spill, I64, &spill_i64, &meta_i64);
    let spill_ptr = ctx.block().inttoptr(I64, &safe_spill_i64);
    let spill_len = ctx.block().load(I32, &spill_ptr);
    let spill_len_i64 = ctx.block().zext(I32, &spill_len, I64);
    let spill_in_bounds = ctx.block().icmp_ult(I64, &object_slot, &spill_len_i64);
    let spill_ok = ctx.block().and(I1, &has_spill, &spill_in_bounds);
    ctx.block()
        .cond_br(&spill_ok, &object_spill_load_label, &object_miss_label);

    ctx.current_block = object_spill_load_idx;
    let spill_element_word = ctx.block().add(I64, &object_slot, "1");
    let spill_element_ptr =
        ctx.block()
            .gep_inbounds(I64, &spill_ptr, &[(I64, &spill_element_word)]);
    let spill_raw = ctx.block().load(DOUBLE, &spill_element_ptr);
    let spill_value = if coerce_slow_to_number {
        ctx.block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &spill_raw)])
    } else {
        spill_raw
    };
    let spill_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = object_miss_idx;
    let slow_raw = ctx.block().call(
        DOUBLE,
        "js_packed_arraylike_index_get",
        &[(DOUBLE, obj_box), (DOUBLE, idx_d), (PTR, &cache_ref)],
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

    kind_incoming.push((inline_value, inline_end_label));
    kind_incoming.push((spill_value, spill_end_label));

    // ---- final merge: one phi over every per-kind fast end + the slow end ----
    ctx.current_block = merge_idx;
    let mut incoming_refs: Vec<(&str, &str)> = kind_incoming
        .iter()
        .map(|(v, l)| (v.as_str(), l.as_str()))
        .collect();
    incoming_refs.push((slow_val.as_str(), slow_end_label.as_str()));
    ctx.block().phi(DOUBLE, &incoming_refs)
}

/// Emit one per-kind small-integer (1/2-byte) typed-array element load block for
/// [`lower_inline_dyn_typed_array_get`]: switches to `blk_idx`, computes the
/// element address (`data_base + (idx << shift)`), loads `elem_ty`, sign-/zero-
/// extends to i32, converts to f64, and branches to `merge_label`. Returns the
/// `(value, end_label)` pair for the merge phi.
#[allow(clippy::too_many_arguments)]
fn emit_inline_ta_int_load(
    ctx: &mut FnCtx<'_>,
    blk_idx: usize,
    idx_i64: &str,
    data_base: &str,
    merge_label: &str,
    shift: &str,
    elem_ty: crate::types::LlvmType,
    signed: bool,
) -> (String, String) {
    ctx.current_block = blk_idx;
    let blk = ctx.block();
    let off = blk.shl(I64, idx_i64, shift);
    let addr = blk.add(I64, data_base, &off);
    let ptr = blk.inttoptr(I64, &addr);
    let raw_elem = blk.load(elem_ty, &ptr);
    let val = if signed {
        let i32v = blk.sext(elem_ty, &raw_elem, I32);
        blk.sitofp(I32, &i32v, DOUBLE)
    } else {
        let i32v = blk.zext(elem_ty, &raw_elem, I32);
        blk.uitofp(I32, &i32v, DOUBLE)
    };
    let end_label = blk.label.clone();
    blk.br(merge_label);
    (val, end_label)
}
