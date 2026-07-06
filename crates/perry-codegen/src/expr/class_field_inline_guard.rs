//! #5093: codegen-inlined class-field shape guard.
//!
//! Monomorphic `this.field` reads/writes on a known class instance previously
//! routed every access through a cross-crate
//! `js_typed_feedback_class_field_{get,set}_guard` *call* before touching the
//! raw slot. Measurements in #5093 showed the call itself — not its body — was
//! the dominant cost on the `09_method_calls` benchmark (~290× Node). This
//! emits the cheap part of the guard's contract as inline IR: when the
//! monomorphic shape holds (and, for raw-f64 fields, the per-object typed-layout
//! intact bit is set), control branches straight to the fast slot load/store,
//! skipping the call. Because every operand is loaded from a loop-invariant
//! receiver, once the surrounding method is inlined (#5092) LLVM LICM can hoist
//! the whole shape check out of the hot loop, collapsing the body to a bare
//! `load`/`fadd`/`store`.
//!
//! The inline check is a strict subset of `class_field_fast_contract` (runtime
//! `typed_feedback/guards.rs`): if it passes, the guard call would have returned
//! "fast". On any miss it falls through to the unchanged guard-call path, so the
//! optimization is purely additive — it can never take the fast path the guard
//! would have rejected. The single per-object `GC_OBJ_TYPED_LAYOUT_INTACT` bit
//! (runtime `gc/layout.rs`) stands in for the thread-local raw-f64 layout probe:
//! it is set exactly when the object's canonical typed descriptor is installed
//! and cleared on any downgrade, so "intact bit set + class_id/keys match" ⟹
//! "slot K is raw-f64" for any field the class declares as a raw-f64 candidate.

use crate::types::{I1, I16, I32, I64, I8};

use super::FnCtx;

// Mirror of the runtime constants the inline check reproduces. Kept as literal
// decimals because the emitted IR is textual.
const POINTER_TAG_HI16: &str = "32765"; // 0x7FFD — NaN-box tag for heap pointers
const HANDLE_BAND_TOP: &str = "1048575"; // 0x0FFFFF — handles are <= this; objects are above
const GC_TYPE_OBJECT: &str = "2";
const GC_FLAG_FORWARDED_I8: &str = "-128"; // 0x80 as i8
const OBJECT_TYPE_REGULAR: &str = "1";
const TYPED_LAYOUT_INTACT_BIT: &str = "4096"; // GC_OBJ_TYPED_LAYOUT_INTACT (0x1000)
const OBJ_FLAG_FROZEN_BIT: &str = "1"; // OBJ_FLAG_FROZEN (0x01)
const OBJ_FLAG_HAS_DESCRIPTORS_BIT: &str = "2048"; // OBJ_FLAG_HAS_DESCRIPTORS (0x800)
const F64_EXP_MASK: &str = "9218868437227405312"; // 0x7FF0_0000_0000_0000

/// Emit the inline class-field shape pre-check.
///
/// Before calling, the caller must have already created `fast_label` (the slot
/// load/store block) and computed `obj_bits` (i64 bitcast of the receiver
/// NaN-box) and `obj_handle` (the low-48 masked pointer) in a block that
/// dominates everything that follows. On success the emitted IR branches to
/// `fast_label`; on any miss it branches to a freshly created "guardcall" block.
///
/// Returns the guardcall block's label and leaves `ctx.current_block` set to it,
/// so the caller emits the unchanged `js_typed_feedback_class_field_*_guard`
/// call path next.
///
/// `set_value_bits` is `Some(bits)` only for the property-set raw-f64 path: it
/// adds the not-frozen and plain-finite-number checks the set fast contract
/// requires (a non-number must downgrade through the boxed setter, never a raw
/// store).
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_class_field_inline_precheck(
    ctx: &mut FnCtx,
    obj_bits: &str,
    obj_handle: &str,
    expected_class_id: &str,
    expected_keys: &str,
    field_index: u32,
    require_raw_f64: bool,
    set_value_bits: Option<&str>,
    fast_label: &str,
) -> String {
    let deref_idx = ctx.new_block("class_field_inline.deref");
    let guardcall_idx = ctx.new_block("class_field_inline.guardcall");
    let deref_label = ctx.block_label(deref_idx);
    let guardcall_label = ctx.block_label(guardcall_idx);
    let field_index_str = field_index.to_string();

    // Gate the dereference: a basic block has no short-circuit, so the field
    // loads below must only run once we know (a) the inline path is enabled and
    // (b) the receiver is a real heap object (POINTER_TAG and above the handle
    // band). Otherwise fall to the guard call, which classifies non-pointer /
    // handle receivers safely and (under PERRY_VERIFY_TYPED_INTACT) runs the
    // intact-bit verifier.
    //
    // The enable flag is checked *first* so the escape hatch
    // (PERRY_DISABLE_CLASS_FIELD_INLINE) and verify mode cleanly bypass the
    // inline reads entirely. It is a `volatile` load: the runtime flips it
    // (sticky 0 -> 1) the moment descriptors / typed-feedback come into use, so
    // LLVM must not hoist a stale 0 across a mid-execution flip — matching the
    // relaxed-atomic read the guard itself performs.
    {
        let blk = ctx.block();
        let flag = blk.load_volatile(I8, "@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED");
        let flag_ok = blk.icmp_eq(I8, &flag, "0");
        let tag = blk.lshr(I64, obj_bits, "48");
        let is_ptr = blk.icmp_eq(I64, &tag, POINTER_TAG_HI16);
        let above_band = blk.icmp_ugt(I64, obj_handle, HANDLE_BAND_TOP);
        let ptr_safe = blk.and(I1, &is_ptr, &above_band);
        let can_inline = blk.and(I1, &ptr_safe, &flag_ok);
        blk.cond_br(&can_inline, &deref_label, &guardcall_label);
    }

    ctx.current_block = deref_idx;
    {
        let blk = ctx.block();
        let obj_ptr = blk.inttoptr(I64, obj_handle);

        // GcHeader (precedes the object by 8 bytes): obj_type @-8 (i8),
        // gc_flags @-7 (i8), _reserved @-6 (i16).
        let gtype_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-8")]);
        let gtype = blk.load(I8, &gtype_ptr);
        let gtype_ok = blk.icmp_eq(I8, &gtype, GC_TYPE_OBJECT);

        let gflags_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-7")]);
        let gflags = blk.load(I8, &gflags_ptr);
        let fwd = blk.and(I8, &gflags, GC_FLAG_FORWARDED_I8);
        let not_fwd = blk.icmp_eq(I8, &fwd, "0");

        let res_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-6")]);
        let reserved = blk.load(I16, &res_ptr);

        // ObjectHeader: object_type @0 (i32)==REGULAR, class_id @4 (i32),
        // field_count @12 (i32), keys_array @16 (i64).
        let object_type = blk.load(I32, &obj_ptr);
        let ot_ok = blk.icmp_eq(I32, &object_type, OBJECT_TYPE_REGULAR);

        let cid_ptr = blk.gep(I8, &obj_ptr, &[(I64, "4")]);
        let class_id = blk.load(I32, &cid_ptr);
        let cid_ok = blk.icmp_eq(I32, &class_id, expected_class_id);

        let fc_ptr = blk.gep(I8, &obj_ptr, &[(I64, "12")]);
        let field_count = blk.load(I32, &fc_ptr);
        let fc_ok = blk.icmp_ugt(I32, &field_count, &field_index_str);

        let ka_ptr = blk.gep(I8, &obj_ptr, &[(I64, "16")]);
        let keys_array = blk.load(I64, &ka_ptr);
        let ka_ok = blk.icmp_eq(I64, &keys_array, expected_keys);

        // (The process-global enable flag was already checked at the gate above,
        // before this dereference.)
        let mut acc = blk.and(I1, &gtype_ok, &not_fwd);
        acc = blk.and(I1, &acc, &ot_ok);
        acc = blk.and(I1, &acc, &cid_ok);
        acc = blk.and(I1, &acc, &fc_ok);
        acc = blk.and(I1, &acc, &ka_ok);

        // #5654: a receiver that has ever had a property / accessor descriptor
        // installed on it (Object.defineProperty / freeze / seal) needs the
        // guard's descriptor-aware dispatch — an accessor must fire, a
        // non-writable slot must reject the store. The per-object flag lets the
        // process-global gate above stay open for such installs (only
        // prototype-level descriptors flip it), so unrelated instances keep the
        // fast path.
        let has_desc = blk.and(I16, &reserved, OBJ_FLAG_HAS_DESCRIPTORS_BIT);
        let no_desc = blk.icmp_eq(I16, &has_desc, "0");
        acc = blk.and(I1, &acc, &no_desc);

        if require_raw_f64 {
            // The slot is read/written as a raw double, so the per-object typed
            // layout must be intact (no downgrade to a NaN-boxed value).
            let intact = blk.and(I16, &reserved, TYPED_LAYOUT_INTACT_BIT);
            let intact_ok = blk.icmp_ne(I16, &intact, "0");
            acc = blk.and(I1, &acc, &intact_ok);
        }

        if let Some(value_bits) = set_value_bits {
            // Frozen objects must route through the boxed setter (which is a
            // no-op for frozen instances), never a raw store.
            let frozen = blk.and(I16, &reserved, OBJ_FLAG_FROZEN_BIT);
            let not_frozen = blk.icmp_eq(I16, &frozen, "0");
            acc = blk.and(I1, &acc, &not_frozen);

            if require_raw_f64 {
                // Only a plain finite number may be stored raw. Non-finite
                // (exponent all-ones: ±Inf/NaN — rare) and every NaN-boxed tag
                // share the all-ones exponent, so a single mask/compare both
                // keeps the fast path correct and routes the boxed/downgrade
                // cases to the guard call.
                let exp = blk.and(I64, value_bits, F64_EXP_MASK);
                let finite = blk.icmp_ne(I64, &exp, F64_EXP_MASK);
                acc = blk.and(I1, &acc, &finite);
            }
        }

        blk.cond_br(&acc, fast_label, &guardcall_label);
    }

    ctx.current_block = guardcall_idx;
    guardcall_label
}
