//! The inline receiver guards of a compiled class-method site: the exact
//! birth-pair compare and the site's learned receiver word.
//!
//! Split out of `method_override.rs` for the 2000-line file-size gate. The
//! header constants and their anti-drift tests stay there.

use super::method_override::{
    GC_OBJECT_LEARNED_GUARD_MASK_I32, GC_OBJECT_METHOD_GUARD_MASK_I32, GC_TYPE_OBJECT,
    POINTER_TAG_HI16, SHAPE_ID_BASE_NEG_I32, SHAPE_ID_RANGE_LEN,
};
use crate::expr::FnCtx;
use crate::types::{I1, I32, I64, I8};

/// Emit the single-arm equivalent of the runtime `js_method_direct_shape_guard`
/// inline: the prototype guard bytes, a pointer gate, then the exact
/// `(class_id, ShapeId)` compare against the birth pair and, when `learned` is
/// given, against the site's learned word (see [`emit_direct_method_site_word`]).
pub(crate) fn emit_inline_direct_method_shape_guard(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    expected_class_id: &str,
    expected_shape_id: &str,
    method_guard_slot: &str,
    fast_label: &str,
    fallback_label: &str,
    // `true` at sites whose receiver may arrive in the internal raw-address
    // form (top word zero); `false` where the receiver is a user-visible
    // NaN-box, so that a plain double whose bit pattern happens to land in
    // the heap address range (a positive subnormal) can never reach the
    // header load — it misses to the runtime guard instead.
    accept_raw_ptr: bool,
    // The site's learned receiver word and the block that calls the method
    // body for it (see [`emit_direct_method_site_word`]). A receiver whose
    // exact pair is not the birth pair is compared against the word before
    // `fallback_label` is taken.
    learned: Option<(&str, &str)>,
) {
    let deref_idx = ctx.new_block("method_direct.inline_deref");
    let deref_label = ctx.block_label(deref_idx);
    let heap_floor =
        crate::target_layout::heap_addr_lower_bound_inclusive(ctx.target_triple).to_string();
    let heap_ceiling =
        crate::target_layout::heap_addr_upper_bound_exclusive(ctx.target_triple).to_string();

    {
        let blk = ctx.block();
        let invalidated =
            blk.load_atomic_acquire(I8, "@PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED", 1);
        let all_methods_ok = blk.icmp_eq(I8, &invalidated, "0");
        let method_slot_ptr = blk.gep(
            I8,
            "@PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD",
            &[(I64, method_guard_slot)],
        );
        let method_invalidated = blk.load_atomic_acquire(I8, &method_slot_ptr, 1);
        let method_ok = blk.icmp_eq(I8, &method_invalidated, "0");
        let prototype_ok = blk.and(I1, &all_methods_ok, &method_ok);
        let recv_bits = blk.bitcast_double_to_i64(recv_box);
        let recv_handle = blk.and(I64, &recv_bits, crate::nanbox::POINTER_MASK_I64);
        let tag = blk.lshr(I64, &recv_bits, "48");
        let is_tagged_ptr = blk.icmp_eq(I64, &tag, POINTER_TAG_HI16);
        // Internal method ABIs also carry an unboxed raw object address in a
        // double-sized slot. `normalize_raw_object_addr` accepts exactly this
        // top-word-zero form; all other non-pointer NaN-box tags remain
        // rejected before dereference.
        let is_ptr = if accept_raw_ptr {
            let is_raw_ptr = blk.icmp_eq(I64, &tag, "0");
            blk.or(I1, &is_tagged_ptr, &is_raw_ptr)
        } else {
            is_tagged_ptr
        };
        let above_floor = blk.icmp_uge(I64, &recv_handle, &heap_floor);
        let below_ceiling = blk.icmp_ult(I64, &recv_handle, &heap_ceiling);
        let in_heap_range = blk.and(I1, &above_floor, &below_ceiling);
        let ptr_safe = blk.and(I1, &is_ptr, &in_heap_range);
        let can_deref = blk.and(I1, &prototype_ok, &ptr_safe);
        blk.cond_br(&can_deref, &deref_label, fallback_label);
    }

    ctx.current_block = deref_idx;
    {
        let blk = ctx.block();
        let recv_bits = blk.bitcast_double_to_i64(recv_box);
        let recv_handle = blk.and(I64, &recv_bits, crate::nanbox::POINTER_MASK_I64);
        let obj_ptr = blk.inttoptr(I64, &recv_handle);

        let gc_header_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-8")]);
        let gc_header = blk.load(I32, &gc_header_ptr);
        let guarded_gc_bits = blk.and(I32, &gc_header, GC_OBJECT_METHOD_GUARD_MASK_I32);
        let gc_header_ok = blk.icmp_eq(I32, &guarded_gc_bits, GC_TYPE_OBJECT);

        // ObjectHeader begins with adjacent `class_id: u32` and
        // `shape_id: u32`. Compare them as one packed word. The expected
        // ShapeId is still range-checked below, so equality proves the live
        // receiver's class is non-zero and its ShapeId is in-domain without
        // four separate field predicates.
        let class_shape = blk.load(I64, &obj_ptr);
        let expected_shape_i64 = blk.zext(I32, expected_shape_id, I64);
        let expected_shape_high = blk.shl(I64, &expected_shape_i64, "32");
        let expected_class_shape = blk.or(I64, &expected_shape_high, expected_class_id);
        let class_shape_ok = crate::typed_shape::emit_compatible_class_shape_eq(
            blk,
            &class_shape,
            expected_class_id,
            expected_shape_id,
            &expected_class_shape,
            &[],
        );

        // `is_shape_id` is `[0x8000_0000, 0xC000_0000)`. Subtract the base
        // modulo i32 and compare with the range length, matching the runtime
        // helper. Equality above transfers this proof to the live header.
        let shape_id_rel = blk.add(I32, expected_shape_id, SHAPE_ID_BASE_NEG_I32);
        let shape_valid = blk.icmp_ult(I32, &shape_id_rel, SHAPE_ID_RANGE_LEN);

        let pass = blk.and(I1, &gc_header_ok, &class_shape_ok);
        let pass = blk.and(I1, &pass, &shape_valid);
        match learned {
            None => blk.cond_br(&pass, fast_label, fallback_label),
            Some((site_word, learned_label)) => {
                let learned_idx = ctx.new_block("method_direct.learned_check");
                let check_label = ctx.block_label(learned_idx);
                ctx.block().cond_br(&pass, fast_label, &check_label);
                ctx.current_block = learned_idx;
                let blk = ctx.block();
                // The word is a fact of one exact `(class_id, ShapeId)` pair
                // (`perry-runtime` `native_call_method/direct_site.rs`); the
                // header bits are re-proved here exactly as for the birth pair.
                let learned_gc_bits = blk.and(I32, &gc_header, GC_OBJECT_LEARNED_GUARD_MASK_I32);
                let learned_gc_ok = blk.icmp_eq(I32, &learned_gc_bits, GC_TYPE_OBJECT);
                let word = blk.load_atomic_monotonic(I64, site_word, 8);
                let word_ok = blk.icmp_eq(I64, &class_shape, &word);
                let hit = blk.and(I1, &learned_gc_ok, &word_ok);
                blk.cond_br(&hit, learned_label, fallback_label);
            }
        }
    }
}

/// One `i64` owned by a compiled class-method site: the last receiver word
/// (`class_id | ShapeId << 32`) the runtime proved carries no own property of
/// the method name and belongs to the declared class
/// (`js_native_call_method_by_id_learn`, `js_object_get_own_field_or_undef_learn`).
/// It starts all-ones, which no header word can equal (a ShapeId is below
/// `0xC000_0000`), and which differs from the `(0, 0)` a declining multi-arm
/// probe yields and from the header word of an unshaped object. Scalars only,
/// so it is not a GC root.
pub(crate) fn emit_direct_method_site_word(ctx: &mut FnCtx<'_>) -> String {
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let prefix = ctx.strings.module_prefix();
    let slot_name = if prefix.is_empty() {
        format!("perry_mdirect_site_{site_id}")
    } else {
        format!("perry_mdirect_site_{prefix}__{site_id}")
    };
    ctx.typed_parse_rodata
        .push(format!("@{slot_name} = private global i64 -1, align 8"));
    format!("@{slot_name}")
}

/// [`emit_direct_method_site_word`] followed by the site's chain memo slot
/// (null, see [`emit_chain_memo_slot`]): `{ i64 word, ptr memo }`. The
/// emitted code reads only the word; the miss edge
/// (`js_native_call_method_by_id_learn`) finds the memo slot next to it.
pub(crate) fn emit_direct_method_site_word_with_memo(ctx: &mut FnCtx<'_>) -> String {
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let prefix = ctx.strings.module_prefix();
    let slot_name = if prefix.is_empty() {
        format!("perry_mdirect_site_{site_id}")
    } else {
        format!("perry_mdirect_site_{prefix}__{site_id}")
    };
    ctx.typed_parse_rodata.push(format!(
        "@{slot_name} = private global {{ i64, ptr }} {{ i64 -1, ptr null }}, align 8"
    ));
    format!("@{slot_name}")
}

/// One pointer owned by a by-name method call site: the slot of its chain
/// memo (`perry-runtime` `object/method_site/chain_memo.rs`), which the
/// runtime allocates on the site's first class-method answer and which
/// records the prototype walk that answered it, validated by ShapeId on every
/// use. Starts null; emitted code never reads it.
pub(crate) fn emit_chain_memo_slot(ctx: &mut FnCtx<'_>) -> String {
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let prefix = ctx.strings.module_prefix();
    let slot_name = if prefix.is_empty() {
        format!("perry_cmemo_{site_id}")
    } else {
        format!("perry_cmemo_{prefix}__{site_id}")
    };
    ctx.typed_parse_rodata
        .push(format!("@{slot_name} = private global ptr null, align 8"));
    format!("@{slot_name}")
}

/// `i1`: `recv_box` is a heap object of `GC_TYPE_OBJECT`, not forwarded, whose
/// exact receiver word equals the site's learned word. Emits
/// its own pointer gate, so it is safe for any value.
pub(super) fn emit_learned_word_hit(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    site_word: &str,
) -> String {
    let deref_idx = ctx.new_block("learned_word.deref");
    let merge_idx = ctx.new_block("learned_word.merge");
    let deref_label = ctx.block_label(deref_idx);
    let merge_label = ctx.block_label(merge_idx);
    let heap_floor =
        crate::target_layout::heap_addr_lower_bound_inclusive(ctx.target_triple).to_string();
    let heap_ceiling =
        crate::target_layout::heap_addr_upper_bound_exclusive(ctx.target_triple).to_string();
    let gate_label = {
        let blk = ctx.block();
        let recv_bits = blk.bitcast_double_to_i64(recv_box);
        let recv_handle = blk.and(I64, &recv_bits, crate::nanbox::POINTER_MASK_I64);
        let tag = blk.lshr(I64, &recv_bits, "48");
        let is_ptr = blk.icmp_eq(I64, &tag, POINTER_TAG_HI16);
        let above_floor = blk.icmp_uge(I64, &recv_handle, &heap_floor);
        let below_ceiling = blk.icmp_ult(I64, &recv_handle, &heap_ceiling);
        let in_heap_range = blk.and(I1, &above_floor, &below_ceiling);
        let can_deref = blk.and(I1, &is_ptr, &in_heap_range);
        blk.cond_br(&can_deref, &deref_label, &merge_label);
        blk.label.clone()
    };
    ctx.current_block = deref_idx;
    let (hit, deref_end) = {
        let blk = ctx.block();
        let recv_bits = blk.bitcast_double_to_i64(recv_box);
        let recv_handle = blk.and(I64, &recv_bits, crate::nanbox::POINTER_MASK_I64);
        let obj_ptr = blk.inttoptr(I64, &recv_handle);
        let gc_header_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-8")]);
        let gc_header = blk.load(I32, &gc_header_ptr);
        let guarded_gc_bits = blk.and(I32, &gc_header, GC_OBJECT_LEARNED_GUARD_MASK_I32);
        let gc_header_ok = blk.icmp_eq(I32, &guarded_gc_bits, GC_TYPE_OBJECT);
        let class_shape = blk.load(I64, &obj_ptr);
        let word = blk.load_atomic_monotonic(I64, site_word, 8);
        let word_ok = blk.icmp_eq(I64, &class_shape, &word);
        let hit = blk.and(I1, &gc_header_ok, &word_ok);
        blk.br(&merge_label);
        (hit, blk.label.clone())
    };
    ctx.current_block = merge_idx;
    ctx.block()
        .phi(I1, &[("false", &gate_label), (&hit, &deref_end)])
}
