//! The inline receiver guard of a compiled class-method site: the exact
//! birth-pair compare.
//!
//! Split out of `method_override.rs` for the 2000-line file-size gate. The
//! header constants and their anti-drift tests stay there.

use super::method_override::{
    GC_OBJECT_METHOD_GUARD_MASK_I32, GC_TYPE_OBJECT, POINTER_TAG_HI16, SHAPE_ID_BASE_NEG_I32,
    SHAPE_ID_RANGE_LEN,
};
use crate::expr::FnCtx;
use crate::types::{I1, I32, I64, I8};

/// Emit the single-arm equivalent of the runtime `js_method_direct_shape_guard`
/// inline: the holder shape proof, a pointer gate, then the exact
/// `(class_id, ShapeId)` compare against the birth pair.
pub(crate) fn emit_inline_direct_method_shape_guard(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    expected_class_id: &str,
    expected_shape_id: &str,
    holder_ok: &str,
    fast_label: &str,
    fallback_label: &str,
    // `true` at sites whose receiver may arrive in the internal raw-address
    // form (top word zero); `false` where the receiver is a user-visible
    // NaN-box, so that a plain double whose bit pattern happens to land in
    // the heap address range (a positive subnormal) can never reach the
    // header load — it misses to the runtime guard instead.
    accept_raw_ptr: bool,
    // `(family layout id, label)`: the receiver class's compiled births carry
    // that layout id as their ShapeId word (`new_alloc::class_born_on_family`).
    // A miss of the birth pair whose word is `(class_id, family id)` takes
    // `label`, which calls the ordinary method body: the word proves the
    // class and that no own key outside the family's keys exists, which is
    // all method identity needs. It proves no slot offsets.
    family_arm: Option<(&str, &str)>,
) {
    let deref_idx = ctx.new_block("method_direct.inline_deref");
    let deref_label = ctx.block_label(deref_idx);
    let heap_floor =
        crate::target_layout::heap_addr_lower_bound_inclusive(ctx.target_triple).to_string();
    let heap_ceiling =
        crate::target_layout::heap_addr_upper_bound_exclusive(ctx.target_triple).to_string();

    {
        let blk = ctx.block();
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
        let can_deref = blk.and(I1, holder_ok, &ptr_safe);
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
        let Some((family_id, family_label)) = family_arm else {
            blk.cond_br(&pass, fast_label, fallback_label);
            return;
        };
        let family_idx = ctx.new_block("method_direct.family");
        let family_test = ctx.block_label(family_idx);
        ctx.block().cond_br(&pass, fast_label, &family_test);
        ctx.current_block = family_idx;
        let blk = ctx.block();
        let family_i64 = blk.zext(I32, family_id, I64);
        let family_high = blk.shl(I64, &family_i64, "32");
        let expected_family_word = blk.or(I64, &family_high, expected_class_id);
        let family_ok = blk.icmp_eq(I64, &class_shape, &expected_family_word);
        // 0: the runtime could not register the layout (then no birth uses it).
        let registered = blk.icmp_ne(I32, family_id, "0");
        let family_ok = blk.and(I1, &family_ok, &registered);
        let family_ok = blk.and(I1, &family_ok, &gc_header_ok);
        blk.cond_br(&family_ok, family_label, fallback_label);
    }
}
