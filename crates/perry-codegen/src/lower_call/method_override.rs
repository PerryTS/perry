//! Issue #620 own-method-override runtime check.
//!
//! Extracted from `lower_call.rs` (#1099, part of #1097) — pure move,
//! no behavior change. `emit_own_method_override_check` emits a runtime
//! guard before a static class-method dispatch so a `this.method = X`
//! own-property override (or `class X { method = fn; }`) is honored.

use crate::expr::{
    emit_typed_feedback_register_site, i32_bool_to_nanbox, i32_to_nanbox, FnCtx,
    TypedFeedbackContract, TypedFeedbackKind,
};
use crate::nanbox::double_literal;
use crate::native_value::LoweredValue;
use crate::types::{DOUBLE, I1, I32, I64, I8};

const POINTER_TAG_HI16: &str = "32765"; // 0x7FFD
const GC_TYPE_OBJECT: &str = "2";
// The first four bytes before an ObjectHeader are, in little-endian order,
// `gtype: u8`, `flags: u8`, and `reserved: u16`.  One masked i32 load can
// therefore prove the three fields the direct-method contract consumes:
//
//   gtype == GC_TYPE_OBJECT
//   flags & GC_FLAG_FORWARDED == 0
//   reserved & OBJ_FLAG_HAS_DESCRIPTORS == 0
//
// Mask: 0x0800_0000 (descriptor bit) | 0x0000_8000 (forwarded bit) |
// 0x0000_00ff (the complete gtype byte).
const GC_OBJECT_METHOD_GUARD_MASK_I32: &str = "134250751"; // 0x0800_80ff
const SHAPE_ID_BASE_NEG_I32: &str = "-2147483648"; // subtract 0x8000_0000
const SHAPE_ID_RANGE_LEN: &str = "1073741824"; // 0x4000_0000

/// Emit the single-arm equivalent of `js_method_direct_shape_guard` directly
/// into the generated module. The guard remains dynamic at every call site:
/// arbitrary callback code may replace a prototype method or mutate the
/// receiver between loop iterations.
///
/// The first block proves that the value is a tagged heap pointer or the raw
/// object-address form used by internal method ABIs before any dereference.
/// The second block reproduces the runtime helper's production contract: the
/// all-method escape latch and this method name's invalidation byte are clear,
/// the receiver is a non-forwarded ordinary object without own descriptors,
/// and its exact `(class_id, ShapeId)` pair still matches the
/// compiler-published pair. Any failed proof takes the unchanged dynamic
/// method fallback.
pub(crate) fn emit_inline_direct_method_shape_guard(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    expected_class_id: &str,
    expected_shape_id: &str,
    method_guard_slot: &str,
    fast_label: &str,
    fallback_label: &str,
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
        let is_raw_ptr = blk.icmp_eq(I64, &tag, "0");
        let is_ptr = blk.or(I1, &is_tagged_ptr, &is_raw_ptr);
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
        let class_shape_ok = blk.icmp_eq(I64, &class_shape, &expected_class_shape);

        // `is_shape_id` is `[0x8000_0000, 0xC000_0000)`. Subtract the base
        // modulo i32 and compare with the range length, matching the runtime
        // helper. Equality above transfers this proof to the live header.
        let shape_id_rel = blk.add(I32, expected_shape_id, SHAPE_ID_BASE_NEG_I32);
        let shape_valid = blk.icmp_ult(I32, &shape_id_rel, SHAPE_ID_RANGE_LEN);

        let pass = blk.and(I1, &gc_header_ok, &class_shape_ok);
        let pass = blk.and(I1, &pass, &shape_valid);
        blk.cond_br(&pass, fast_label, fallback_label);
    }
}

#[cfg(test)]
mod packed_guard_tests {
    use super::*;

    /// Anti-drift gate for the runtime fields packed into the i32 load at
    /// `obj - 8`. Keep this alongside the emitter so a flag move changes a
    /// failing test instead of silently weakening a generated guard.
    #[test]
    fn gc_header_mask_preserves_the_direct_method_contract() {
        let obj_type_mask = 0x0000_00ffu32;
        let forwarded = u32::from(0x80u8) << 8;
        let has_descriptors = 0x0800u32 << 16;
        let mask = obj_type_mask | forwarded | has_descriptors;
        let expected = u32::from(2u8);

        assert_eq!(GC_OBJECT_METHOD_GUARD_MASK_I32, mask.to_string());
        assert_eq!(expected & mask, expected);
        assert_ne!((expected | forwarded) & mask, expected);
        assert_ne!((expected | has_descriptors) & mask, expected);
        assert_ne!((expected ^ 1) & mask, expected);
    }

    /// `ObjectHeader::{class_id,parent_class_id}` are adjacent u32 fields.
    /// Perry's supported native targets are little-endian, so one i64 load
    /// sees class in the low word and ShapeId in the high word.
    #[test]
    fn class_shape_word_uses_the_supported_little_endian_layout() {
        let class_id = 0x1234_5678u32;
        let shape_id = 0x89ab_cdefu32;
        let mut bytes = [0u8; 8];
        bytes[..4].copy_from_slice(&class_id.to_le_bytes());
        bytes[4..].copy_from_slice(&shape_id.to_le_bytes());
        assert_eq!(
            u64::from_le_bytes(bytes),
            (u64::from(shape_id) << 32) | u64::from(class_id)
        );

        for triple in [
            "aarch64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "aarch64-linux-android",
            "x86_64-pc-windows-msvc",
        ] {
            assert!(!triple.starts_with("s390") && !triple.starts_with("powerpc64-"));
        }
    }
}

fn typed_i1_method_signature_note(reps: &[crate::codegen::TypedParamRep]) -> String {
    let first = reps.first().map(|rep| rep.label()).unwrap_or("void");
    if reps.len() <= 1 {
        format!("typed_signature=i1({first})->i1")
    } else {
        format!("typed_signature=i1({first}, ...)->i1")
    }
}

fn typed_method_signature_note(ret: &str, reps: &[crate::codegen::TypedParamRep]) -> String {
    let first = reps.first().map(|rep| rep.label()).unwrap_or("void");
    if reps.len() <= 1 {
        format!("typed_signature={ret}({first})->{ret}")
    } else {
        format!("typed_signature={ret}({first}, ...)->{ret}")
    }
}

/// Issue #620: emit a runtime check before the static class-method dispatch.
/// If the receiver has an own-property override at `property` (set via
/// `this.method = X`), invoke the stored closure via `js_native_call_value`;
/// otherwise call the static method body directly. Returns the LLVM register
/// holding the unified result (phi over the two branches).
/// `override_user_args` are the FLAT (un-rest-bundled) user arguments — i.e.
/// the source-level call arguments WITHOUT the leading `this` and WITHOUT the
/// trailing rest array the static ABI bundles. The override branch dispatches a
/// dynamic value (an arrow / bound function / native method) via
/// `js_native_call_value`, which performs its own arity/rest handling from a
/// flat positional buffer — so it must receive the spread-out args, not the
/// rest array as one positional. (`super.emit(event, ...args)` forwarding to a
/// native EventEmitter override otherwise delivered `[payload]` to listeners.)
/// The static branch keeps `fallback_arg_slices` (rest-bundled) unchanged.
pub(super) fn emit_own_method_override_check(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    property: &str,
    fallback_fn: &str,
    fallback_arg_slices: &[(crate::types::LlvmType, &str)],
    this_box: &str,
    override_user_args: &[String],
) -> String {
    // Intern the property name so we can pass (ptr, len) directly to the
    // override probe — saves an allocation vs synthesizing a StringHeader.
    let key_idx = ctx.strings.intern(property);
    let entry = ctx.strings.entry(key_idx);
    let bytes_global = format!("@{}", entry.bytes_global);
    let name_len_str = entry.byte_len.to_string();

    let blk = ctx.block();
    let own_method = blk.call(
        DOUBLE,
        "js_object_get_own_field_or_undef",
        &[
            (DOUBLE, recv_box),
            (crate::types::PTR, &bytes_global),
            (I64, &name_len_str),
        ],
    );
    let own_bits = ctx.block().bitcast_double_to_i64(&own_method);
    let undef_bits_str = format!("{}", crate::nanbox::TAG_UNDEFINED as i64);
    let is_undef = ctx.block().icmp_eq(I64, &own_bits, &undef_bits_str);

    let override_idx = ctx.new_block("ovrcheck.override");
    let static_idx = ctx.new_block("ovrcheck.static");
    let merge_idx = ctx.new_block("ovrcheck.merge");
    let override_label = ctx.block_label(override_idx);
    let static_label = ctx.block_label(static_idx);
    let merge_label = ctx.block_label(merge_idx);

    ctx.block()
        .cond_br(&is_undef, &static_label, &override_label);

    // Override path: spill the user args (skip lowered_args[0] which is
    // `this`) into a fresh alloca and call js_native_call_value. The
    // override may be an arrow / `.bind(...)`-bound function whose
    // `this` is captured/bound — but it can also be a regular function
    // assigned via `this.method = fn` or `class X { method = fn; }`
    // (hono's RegExpRouter uses this exact shape — `match = match;`
    // assigns the imported standalone `match` function as an instance
    // own-property; its body reads `this.buildAllMatchers()`). Bind
    // `IMPLICIT_THIS` to the receiver around the call so non-arrow
    // function bodies see the right `this` (issue #632 / #519 pattern).
    ctx.current_block = override_idx;
    let user_arg_count = override_user_args.len();
    let (args_ptr, args_len) = if user_arg_count == 0 {
        ("null".to_string(), "0".to_string())
    } else {
        let buf_reg = ctx.func.alloca_entry_array(DOUBLE, user_arg_count);
        for (i, a_val) in override_user_args.iter().enumerate() {
            let slot = ctx
                .block()
                .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
            ctx.block().store(DOUBLE, a_val, &slot);
        }
        let ptr_reg = ctx.block().next_reg();
        ctx.block().emit_raw(format!(
            "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
            ptr_reg, user_arg_count, buf_reg
        ));
        (ptr_reg, user_arg_count.to_string())
    };
    let recv_for_this = if this_box.is_empty() {
        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
    } else {
        this_box.to_string()
    };
    // #7211: rooted save/restore — the displaced implicit `this` is live
    // across `js_native_call_value`, which runs arbitrary user code.
    let prev_this = crate::rooting::implicit_this_save(ctx, &recv_for_this);
    let v_override = ctx.block().call(
        DOUBLE,
        "js_native_call_value",
        &[
            (DOUBLE, &own_method),
            (crate::types::PTR, &args_ptr),
            (I64, &args_len),
        ],
    );
    crate::rooting::implicit_this_restore(ctx, prev_this);
    let after_override = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    // Static path: original direct call to fallback_fn.
    ctx.current_block = static_idx;
    let v_static = ctx.block().call(DOUBLE, fallback_fn, fallback_arg_slices);
    let after_static = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (v_override.as_str(), after_override.as_str()),
            (v_static.as_str(), after_static.as_str()),
        ],
    )
}

/// One additional `(class id, keys token) -> concrete method` arm for the
/// shape-guarded direct call, describing a class in the DECLARED receiver
/// class's subclass closure.
///
/// The declared-class guard speculates that the receiver's dynamic class is
/// exactly its static class. For a receiver typed as the base of a hierarchy —
/// `nodes: Node2D[]`, every element a `Rect` / `Circle` / `Square` / `Marker` /
/// `Group` — that speculation is wrong for EVERY element, so the guard misses
/// 100% of the time and each call pays a wasted guard plus the full
/// `js_native_call_method` dispatch tower. Each arm here is the same proof the
/// declared-class guard performs (exact class id + exact keys token), applied
/// to one more class whose implementation of the method codegen already
/// resolved statically.
pub(super) struct SubclassDispatchArm {
    /// `class_id` of the concrete subclass this arm matches.
    pub class_id: u32,
    /// Name of the module global holding that subclass's canonical keys array.
    pub keys_global: String,
    /// The method body `property` resolves to when walked from that subclass.
    pub target_fn: String,
}

/// Emit a typed-feedback runtime guard before a known class-method direct call.
///
/// The guard validates that the receiver still has the expected class shape,
/// has no own-property method replacement, and still resolves the method name
/// to the direct function pointer in the runtime vtable. Failures branch to the
/// existing dynamic method dispatcher and record a fallback once.
pub(super) fn emit_guarded_direct_method_call(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    receiver_class_name: &str,
    property: &str,
    direct_fn: &str,
    direct_arg_slices: &[(crate::types::LlvmType, &str)],
    fallback_user_args: &[String],
    nonnegative_index_direct_fn: Option<&str>,
    typed_direct_fn: Option<(&str, Vec<crate::codegen::TypedParamRep>)>,
    typed_f64_receiver_direct_fn: Option<(&str, usize, &crate::codegen::TypedReceiverMethodInfo)>,
    typed_i32_direct_fn: Option<(&str, Vec<crate::codegen::TypedParamRep>)>,
    typed_i1_direct_fn: Option<(&str, Vec<crate::codegen::TypedParamRep>)>,
    typed_string_direct_fn: Option<(&str, Vec<crate::codegen::TypedParamRep>)>,
    shape_only_guard: bool,
    subclass_arms: &[SubclassDispatchArm],
) -> Option<String> {
    let expected_class_id = *ctx.class_ids.get(receiver_class_name)?;
    let keys_global_name = ctx.class_keys_globals.get(receiver_class_name)?.clone();
    // Only the shape-only guard is widened. The typed-feedback guard records an
    // observation keyed to ONE (class, method, func ptr) contract per site; a
    // multi-class site would feed it a stream of "different class" observations
    // and it would (correctly) mark the site polymorphic. That form keeps its
    // single-arm shape.
    let subclass_arms: &[SubclassDispatchArm] = if shape_only_guard { subclass_arms } else { &[] };

    // Representation-selection Phase 5a: the proven-`this` clone for this
    // (class, method), when the emission loop produced one.
    //
    // Computed ONCE here rather than per-arm because the justification is the
    // same for every block this helper emits below: they are all dominated by
    // the `js_method_direct_shape_guard` /
    // `js_typed_feedback_method_direct_call_guard` branch, which matched the
    // exact class id AND the keys token. A `pshape_methods` hit additionally
    // proves `receiver_class_name` DECLARES `property` (locally by analysis or
    // across modules by a producer-authored capability), so the clone's `this`
    // is exactly the class it was compiled for and can never be a subclass
    // instance.
    //
    // The `perry_static_` exclusion is carried forward from the guard-free
    // site (the #1787 static-receiver bug): those targets need
    // `js_class_static_method_call`, not a plain `call double`, and no
    // proven-`this` clone is ever emitted for them.
    let pshape_fn: Option<String> = (!direct_fn.starts_with("perry_static_")
        && ctx
            .pshape_methods
            .contains_key(&(receiver_class_name.to_string(), property.to_string())))
    .then(|| crate::collectors::pshape_method_name(direct_fn));

    // The body a failed typed guard falls back to. Arm-invariant (both inputs
    // are), so it is resolved once here rather than five times below.
    let generic_body_fn: String = pshape_fn
        .clone()
        .unwrap_or_else(|| crate::codegen::generic_method_body_name(direct_fn));

    let expected_class_id_str = expected_class_id.to_string();
    let expected_shape_id =
        crate::typed_shape::load_class_shape_id(ctx, receiver_class_name, &keys_global_name);

    let key_idx = ctx.strings.intern(property);
    let entry = ctx.strings.entry(key_idx);
    let bytes_global = format!("@{}", entry.bytes_global);
    let name_len_str = entry.byte_len.to_string();
    let method_guard_slot_str = (entry.dispatch_hash & 0xffff).to_string();
    let dispatch_global = ctx.strings.static_dispatch_global(key_idx);
    let site_id = if shape_only_guard {
        None
    } else {
        Some(emit_typed_feedback_register_site(
            ctx,
            TypedFeedbackKind::MethodCall,
            property,
            TypedFeedbackContract::method_direct_call(),
        ))
    };

    // Per-arm ShapeIds, loaded through entry-block scalar slots.
    let subclass_shape_ids: Vec<String> = subclass_arms
        .iter()
        .map(|arm| {
            let shape_global =
                crate::typed_shape::shape_id_global_name_from_keys_global(&arm.keys_global);
            let slot = ctx.func.entry_init_load_global(&shape_global, I32);
            ctx.block().load(I32, &slot)
        })
        .collect();

    let guard_idx = ctx.new_block("method_direct.guard");
    let fast_idx = ctx.new_block("method_direct.fast");
    // One test block and one case block per subclass arm. The declared class's
    // own test lives in the guard block, so arm 0's test block is the guard's
    // false edge.
    let sub_test_idxs: Vec<usize> = (0..subclass_arms.len())
        .map(|i| ctx.new_block(&format!("method_direct.subtest{i}")))
        .collect();
    let sub_case_idxs: Vec<usize> = (0..subclass_arms.len())
        .map(|i| ctx.new_block(&format!("method_direct.sub{i}")))
        .collect();
    let fallback_idx = ctx.new_block("method_direct.fallback");
    let merge_idx = ctx.new_block("method_direct.merge");
    let guard_label = ctx.block_label(guard_idx);
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    let sub_test_labels: Vec<String> = sub_test_idxs.iter().map(|&i| ctx.block_label(i)).collect();
    let sub_case_labels: Vec<String> = sub_case_idxs.iter().map(|&i| ctx.block_label(i)).collect();
    ctx.block().br(&guard_label);

    ctx.current_block = guard_idx;
    // Multi-arm form: ONE probe resolves the receiver's class id and keys
    // token (every precondition `js_method_direct_shape_guard` checks except
    // the comparison itself), then an inline compare chain picks the arm. A
    // shape-only single-arm site emits the equivalent guard inline; other
    // single-arm sites retain the runtime helper.
    let multi_arm = !subclass_arms.is_empty();
    let inline_single_arm = shape_only_guard && !multi_arm;
    if multi_arm {
        let shape_slot = ctx.func.alloca_entry(I32);
        let cid = ctx.block().call(
            I32,
            "js_method_direct_shape_class",
            &[
                (DOUBLE, recv_box),
                (crate::types::PTR, &shape_slot),
                (I32, &method_guard_slot_str),
            ],
        );
        let shape_id = ctx.block().load(I32, &shape_slot);
        {
            let next = sub_test_labels[0].clone();
            let blk = ctx.block();
            let cid_ok = blk.icmp_eq(I32, &cid, &expected_class_id_str);
            let shape_ok = blk.icmp_eq(I32, &shape_id, &expected_shape_id);
            let pass = blk.and(I1, &cid_ok, &shape_ok);
            blk.cond_br(&pass, &fast_label, &next);
        }
        for (i, arm) in subclass_arms.iter().enumerate() {
            ctx.current_block = sub_test_idxs[i];
            let next = sub_test_labels
                .get(i + 1)
                .cloned()
                .unwrap_or_else(|| fallback_label.clone());
            let case_label = sub_case_labels[i].clone();
            let class_id_str = arm.class_id.to_string();
            let arm_shape_id = subclass_shape_ids[i].clone();
            let blk = ctx.block();
            let cid_ok = blk.icmp_eq(I32, &cid, &class_id_str);
            let shape_ok = blk.icmp_eq(I32, &shape_id, &arm_shape_id);
            let pass = blk.and(I1, &cid_ok, &shape_ok);
            blk.cond_br(&pass, &case_label, &next);
        }
        ctx.current_block = guard_idx;
    }
    if inline_single_arm {
        emit_inline_direct_method_shape_guard(
            ctx,
            recv_box,
            &expected_class_id_str,
            &expected_shape_id,
            &method_guard_slot_str,
            &fast_label,
            &fallback_label,
        );
    }
    let guard_ok = if multi_arm || inline_single_arm {
        // The chain above already terminated the guard block and every test
        // block, or the inline single-arm guard terminated both its pointer
        // gate and header block; `fast_idx` / `fallback_idx` are entered from
        // either form unchanged.
        String::new()
    } else if shape_only_guard {
        ctx.block().call(
            I32,
            "js_method_direct_shape_guard",
            &[
                (DOUBLE, recv_box),
                (I32, &expected_class_id_str),
                (I32, &expected_shape_id),
                (I32, &method_guard_slot_str),
            ],
        )
    } else {
        ctx.block().call(
            I32,
            "js_typed_feedback_method_direct_call_guard",
            &[
                (
                    I64,
                    site_id.as_deref().expect("typed-feedback method site id"),
                ),
                (DOUBLE, recv_box),
                (I32, &expected_class_id_str),
                (I32, &expected_shape_id),
                (crate::types::PTR, &bytes_global),
                (I64, &name_len_str),
                (crate::types::PTR, &format!("@{}", direct_fn)),
            ],
        )
    };
    if !multi_arm && !inline_single_arm {
        let guard_pass = ctx.block().icmp_ne(I32, &guard_ok, "0");
        ctx.block()
            .cond_br(&guard_pass, &fast_label, &fallback_label);
    }

    ctx.current_block = fast_idx;
    let fast_value = {
        if let Some((typed_fn, typed_formal_count, receiver_info)) = typed_f64_receiver_direct_fn {
            let formal_args: Vec<&str> = direct_arg_slices
                .iter()
                .skip(1)
                .take(typed_formal_count)
                .map(|(_, value)| *value)
                .collect();
            let mut guard: Option<String> = None;
            for value in &formal_args {
                let raw = ctx
                    .block()
                    .call(I32, "js_typed_f64_arg_guard", &[(DOUBLE, *value)]);
                let ok = ctx.block().icmp_ne(I32, &raw, "0");
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }
            for field in &receiver_info.fields {
                let site_id = emit_typed_feedback_register_site(
                    ctx,
                    TypedFeedbackKind::PropertyGet,
                    &field.name,
                    TypedFeedbackContract::class_field_get(),
                );
                let key_idx = ctx.strings.intern(&field.name);
                let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let key_box = ctx.block().load(DOUBLE, &key_handle_global);
                let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
                let key_raw = ctx
                    .block()
                    .and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
                let field_index_str = field.index.to_string();
                let raw_guard = ctx.block().call(
                    I32,
                    "js_typed_feedback_class_field_get_guard",
                    &[
                        (I64, &site_id),
                        (DOUBLE, recv_box),
                        (I32, &expected_class_id_str),
                        (I32, &expected_shape_id),
                        (I64, &key_raw),
                        (I32, &field_index_str),
                        (I32, "1"),
                    ],
                );
                let ok = ctx.block().icmp_ne(I32, &raw_guard, "0");
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }

            let typed_idx = ctx.new_block("typed_f64_recv_method.fast");
            let generic_idx = ctx.new_block("typed_f64_recv_method.generic");
            let typed_merge_idx = ctx.new_block("typed_f64_recv_method.merge");
            let typed_label = ctx.block_label(typed_idx);
            let generic_label = ctx.block_label(generic_idx);
            let typed_merge_label = ctx.block_label(typed_merge_idx);
            if let Some(guard) = guard {
                ctx.block().cond_br(&guard, &typed_label, &generic_label);
            } else {
                ctx.block().br(&typed_label);
            }

            ctx.current_block = typed_idx;
            let recv_bits = ctx.block().bitcast_double_to_i64(recv_box);
            let recv_handle = ctx
                .block()
                .and(I64, &recv_bits, crate::nanbox::POINTER_MASK_I64);
            let mut typed_args_storage: Vec<String> = Vec::with_capacity(formal_args.len());
            for value in &formal_args {
                typed_args_storage.push(ctx.block().call(
                    DOUBLE,
                    "js_typed_f64_arg_to_raw",
                    &[(DOUBLE, *value)],
                ));
            }
            let mut typed_args: Vec<(crate::types::LlvmType, &str)> =
                Vec::with_capacity(typed_args_storage.len() + 1);
            typed_args.push((I64, recv_handle.as_str()));
            for value in &typed_args_storage {
                typed_args.push((DOUBLE, value.as_str()));
            }
            let typed_value = ctx.block().call(DOUBLE, typed_fn, &typed_args);
            let after_typed = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = generic_idx;
            let generic_value = ctx
                .block()
                .call(DOUBLE, &generic_body_fn, direct_arg_slices);
            let after_generic = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = typed_merge_idx;
            let result = ctx.block().phi(
                DOUBLE,
                &[
                    (typed_value.as_str(), after_typed.as_str()),
                    (generic_value.as_str(), after_generic.as_str()),
                ],
            );
            ctx.record_lowered_value(
                "MethodCall",
                None,
                "typed_f64_receiver_method_direct_call",
                &LoweredValue::f64(result.clone()),
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("typed_clone={typed_fn}"),
                    format!("generic_method={generic_body_fn}"),
                    format!("receiver_class={receiver_class_name}"),
                    format!("method={property}"),
                    "receiver_arg=i64".to_string(),
                    "raw_f64_field_guard=required".to_string(),
                ],
            );
            result
        } else if let Some((typed_fn, typed_param_reps)) = typed_direct_fn {
            let formal_args: Vec<&str> = direct_arg_slices
                .iter()
                .skip(1)
                .take(typed_param_reps.len())
                .map(|(_, value)| *value)
                .collect();
            let mut guard: Option<String> = None;
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), *rep, value);
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }

            let typed_idx = ctx.new_block("typed_f64_method.fast");
            let generic_idx = ctx.new_block("typed_f64_method.generic");
            let typed_merge_idx = ctx.new_block("typed_f64_method.merge");
            let typed_label = ctx.block_label(typed_idx);
            let generic_label = ctx.block_label(generic_idx);
            let typed_merge_label = ctx.block_label(typed_merge_idx);
            if let Some(guard) = guard {
                ctx.block().cond_br(&guard, &typed_label, &generic_label);
            } else {
                ctx.block().br(&typed_label);
            }

            ctx.current_block = typed_idx;
            let mut typed_args_storage: Vec<String> = Vec::with_capacity(formal_args.len());
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                typed_args_storage.push(crate::codegen::emit_typed_arg_to_raw(
                    ctx.block(),
                    *rep,
                    value,
                ));
            }
            let typed_args: Vec<(crate::types::LlvmType, &str)> = typed_args_storage
                .iter()
                .zip(typed_param_reps.iter())
                .map(|(value, rep)| (rep.llvm_ty(), value.as_str()))
                .collect();
            let typed_value = ctx.block().call(DOUBLE, typed_fn, &typed_args);
            let after_typed = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = generic_idx;
            let generic_value = ctx
                .block()
                .call(DOUBLE, &generic_body_fn, direct_arg_slices);
            let after_generic = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = typed_merge_idx;
            let result = ctx.block().phi(
                DOUBLE,
                &[
                    (typed_value.as_str(), after_typed.as_str()),
                    (generic_value.as_str(), after_generic.as_str()),
                ],
            );
            ctx.record_lowered_value(
                "MethodCall",
                None,
                "typed_f64_method_direct_call",
                &LoweredValue::f64(result.clone()),
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("typed_clone={typed_fn}"),
                    format!("generic_method={generic_body_fn}"),
                    format!("receiver_class={receiver_class_name}"),
                    format!("method={property}"),
                    typed_method_signature_note("f64", &typed_param_reps),
                ],
            );
            result
        } else if let Some((typed_fn, typed_param_reps)) = typed_i32_direct_fn {
            let formal_args: Vec<&str> = direct_arg_slices
                .iter()
                .skip(1)
                .take(typed_param_reps.len())
                .map(|(_, value)| *value)
                .collect();
            let mut guard: Option<String> = None;
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), *rep, value);
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }

            let typed_idx = ctx.new_block("typed_i32_method.fast");
            let generic_idx = ctx.new_block("typed_i32_method.generic");
            let typed_merge_idx = ctx.new_block("typed_i32_method.merge");
            let typed_label = ctx.block_label(typed_idx);
            let generic_label = ctx.block_label(generic_idx);
            let typed_merge_label = ctx.block_label(typed_merge_idx);
            if let Some(guard) = guard {
                ctx.block().cond_br(&guard, &typed_label, &generic_label);
            } else {
                ctx.block().br(&typed_label);
            }

            ctx.current_block = typed_idx;
            let mut typed_args_storage: Vec<String> = Vec::with_capacity(formal_args.len());
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                typed_args_storage.push(crate::codegen::emit_typed_arg_to_raw(
                    ctx.block(),
                    *rep,
                    value,
                ));
            }
            let typed_args: Vec<(crate::types::LlvmType, &str)> = typed_args_storage
                .iter()
                .zip(typed_param_reps.iter())
                .map(|(value, rep)| (rep.llvm_ty(), value.as_str()))
                .collect();
            let raw_i32 = ctx.block().call(I32, typed_fn, &typed_args);
            let typed_value = i32_to_nanbox(ctx.block(), &raw_i32);
            let after_typed = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = generic_idx;
            let generic_value = ctx
                .block()
                .call(DOUBLE, &generic_body_fn, direct_arg_slices);
            let after_generic = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = typed_merge_idx;
            let result = ctx.block().phi(
                DOUBLE,
                &[
                    (typed_value.as_str(), after_typed.as_str()),
                    (generic_value.as_str(), after_generic.as_str()),
                ],
            );
            ctx.record_lowered_value(
                "MethodCall",
                None,
                "typed_i32_method_direct_call",
                &LoweredValue::js_value(result.clone()),
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("typed_clone={typed_fn}"),
                    format!("generic_method={generic_body_fn}"),
                    format!("receiver_class={receiver_class_name}"),
                    format!("method={property}"),
                    typed_method_signature_note("i32", &typed_param_reps),
                    "boxed_result_at=direct_call_boundary".to_string(),
                ],
            );
            result
        } else if let Some((typed_fn, typed_param_reps)) = typed_i1_direct_fn {
            let formal_args: Vec<&str> = direct_arg_slices
                .iter()
                .skip(1)
                .take(typed_param_reps.len())
                .map(|(_, value)| *value)
                .collect();
            let mut guard: Option<String> = None;
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                let raw = ctx.block().call(I32, rep.guard_fn(), &[(DOUBLE, *value)]);
                let ok = ctx.block().icmp_ne(I32, &raw, "0");
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }

            let typed_idx = ctx.new_block("typed_i1_method.fast");
            let generic_idx = ctx.new_block("typed_i1_method.generic");
            let typed_merge_idx = ctx.new_block("typed_i1_method.merge");
            let typed_label = ctx.block_label(typed_idx);
            let generic_label = ctx.block_label(generic_idx);
            let typed_merge_label = ctx.block_label(typed_merge_idx);
            if let Some(guard) = guard {
                ctx.block().cond_br(&guard, &typed_label, &generic_label);
            } else {
                ctx.block().br(&typed_label);
            }

            ctx.current_block = typed_idx;
            let mut typed_args_storage: Vec<String> = Vec::with_capacity(formal_args.len());
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                typed_args_storage.push(match rep {
                    crate::codegen::TypedParamRep::F64 => {
                        ctx.block()
                            .call(DOUBLE, rep.unbox_fn(), &[(DOUBLE, *value)])
                    }
                    crate::codegen::TypedParamRep::I32 => {
                        ctx.block().call(I32, rep.unbox_fn(), &[(DOUBLE, *value)])
                    }
                    crate::codegen::TypedParamRep::I1 => {
                        let raw_i32 = ctx.block().call(I32, rep.unbox_fn(), &[(DOUBLE, *value)]);
                        ctx.block().icmp_ne(I32, &raw_i32, "0")
                    }
                    crate::codegen::TypedParamRep::StringRef => {
                        ctx.block().call(I64, rep.unbox_fn(), &[(DOUBLE, *value)])
                    }
                });
            }
            let typed_args: Vec<(crate::types::LlvmType, &str)> = typed_args_storage
                .iter()
                .zip(typed_param_reps.iter())
                .map(|(value, rep)| (rep.llvm_ty(), value.as_str()))
                .collect();
            let typed_i1 = ctx.block().call(I1, typed_fn, &typed_args);
            let typed_i32 = ctx.block().zext(I1, &typed_i1, I32);
            let typed_value = i32_bool_to_nanbox(ctx.block(), &typed_i32);
            let after_typed = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = generic_idx;
            let generic_value = ctx
                .block()
                .call(DOUBLE, &generic_body_fn, direct_arg_slices);
            let after_generic = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = typed_merge_idx;
            let result = ctx.block().phi(
                DOUBLE,
                &[
                    (typed_value.as_str(), after_typed.as_str()),
                    (generic_value.as_str(), after_generic.as_str()),
                ],
            );
            ctx.record_lowered_value(
                "MethodCall",
                None,
                "typed_i1_method_direct_call",
                &LoweredValue::js_value(result.clone()),
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("typed_clone={typed_fn}"),
                    format!("generic_method={generic_body_fn}"),
                    format!("receiver_class={receiver_class_name}"),
                    format!("method={property}"),
                    typed_i1_method_signature_note(&typed_param_reps),
                    "boxed_result_at=direct_call_boundary".to_string(),
                ],
            );
            result
        } else if let Some((typed_fn, typed_param_reps)) = typed_string_direct_fn {
            let formal_args: Vec<&str> = direct_arg_slices
                .iter()
                .skip(1)
                .take(typed_param_reps.len())
                .map(|(_, value)| *value)
                .collect();
            let mut guard: Option<String> = None;
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), *rep, value);
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }

            let typed_idx = ctx.new_block("typed_string_method.fast");
            let generic_idx = ctx.new_block("typed_string_method.generic");
            let typed_merge_idx = ctx.new_block("typed_string_method.merge");
            let typed_label = ctx.block_label(typed_idx);
            let generic_label = ctx.block_label(generic_idx);
            let typed_merge_label = ctx.block_label(typed_merge_idx);
            if let Some(guard) = guard {
                ctx.block().cond_br(&guard, &typed_label, &generic_label);
            } else {
                ctx.block().br(&typed_label);
            }

            ctx.current_block = typed_idx;
            let mut typed_args_storage: Vec<String> = Vec::with_capacity(formal_args.len());
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                typed_args_storage.push(crate::codegen::emit_typed_arg_to_raw(
                    ctx.block(),
                    *rep,
                    value,
                ));
            }
            let typed_args: Vec<(crate::types::LlvmType, &str)> = typed_args_storage
                .iter()
                .zip(typed_param_reps.iter())
                .map(|(value, rep)| (rep.llvm_ty(), value.as_str()))
                .collect();
            let raw_string = ctx.block().call(I64, typed_fn, &typed_args);
            let typed_value = ctx
                .block()
                .call(DOUBLE, "js_nanbox_string", &[(I64, &raw_string)]);
            let after_typed = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = generic_idx;
            let generic_value = ctx
                .block()
                .call(DOUBLE, &generic_body_fn, direct_arg_slices);
            let after_generic = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = typed_merge_idx;
            let result = ctx.block().phi(
                DOUBLE,
                &[
                    (typed_value.as_str(), after_typed.as_str()),
                    (generic_value.as_str(), after_generic.as_str()),
                ],
            );
            ctx.record_lowered_value(
                "MethodCall",
                None,
                "typed_string_method_direct_call",
                &LoweredValue::js_value(result.clone()),
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("typed_clone={typed_fn}"),
                    format!("generic_method={generic_body_fn}"),
                    format!("receiver_class={receiver_class_name}"),
                    format!("method={property}"),
                    typed_method_signature_note("string", &typed_param_reps),
                    "boxed_result_at=direct_call_boundary".to_string(),
                ],
            );
            result
        } else {
            // Representation-selection Phase 5a: this arm is reached ONLY
            // after `js_method_direct_shape_guard` /
            // `js_typed_feedback_method_direct_call_guard` matched the exact
            // class id AND the keys token — i.e. the receiver's shape is
            // already proven, and the proof is then thrown away by calling the
            // guard-ridden public body. Route to the proven-`this` clone
            // instead; identical ABI, so only the callee name changes.
            //
            // A `pshape_methods` hit additionally proves `receiver_class_name`
            // DECLARES `property` (locally by analysis or across modules by a
            // producer-authored capability), so the clone's `this` is exactly
            // the class it was compiled for — an inherited `Base::m` reached
            // through a subclass receiver never routes here.
            //
            // NOTE: the per-field `js_typed_feedback_class_field_get_guard`
            // loop above is deliberately LEFT IN PLACE. It guards the
            // `$typed_f64_recv` clone's bare `load double` field access, and
            // the whole-object shape guard does NOT subsume it: an external
            // `obj.f = "s"` preserves both the class id and the key set while
            // downgrading the slot's raw-f64 layout. The `$pshape` clone
            // needs no such guard because it never claims `JsNumber` — its
            // bare loads carry generic `JsValue` semantics (see
            // `collectors/proven_this.rs`).
            //
            // `pshape_fn` (computed once at the top of this function, where the
            // `perry_static_` exclusion and the declaring-class argument are
            // written out) is the same clone the typed arms above now route
            // their generic fallbacks to.
            let target = nonnegative_index_direct_fn
                .or(pshape_fn.as_deref())
                .unwrap_or(direct_fn);
            let result = ctx.block().call(DOUBLE, target, direct_arg_slices);
            if nonnegative_index_direct_fn.is_none() {
                if let Some(pshape) = pshape_fn.as_deref() {
                    let receiver_provenance =
                        if ctx.imported_class_sources.contains_key(receiver_class_name) {
                            "imported_class_metadata"
                        } else {
                            "module_local_analysis"
                        };
                    ctx.record_lowered_value(
                        "MethodCall",
                        None,
                        "proven_this_method_direct_call",
                        &LoweredValue::js_value(result.clone()),
                        None,
                        None,
                        None,
                        false,
                        false,
                        vec![
                            format!("typed_clone={pshape}"),
                            format!("generic_method={direct_fn}"),
                            format!("receiver_class={receiver_class_name}"),
                            format!("method={property}"),
                            format!("receiver_provenance={receiver_provenance}"),
                            "this_representation=tagged_js_value_exact_shape".to_string(),
                            "method_identity_guard=required".to_string(),
                            "generic_dispatch_fallback=js_native_call_method_by_id".to_string(),
                        ],
                    );
                }
            }
            result
        }
    };
    let after_fast = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    // One direct call per subclass arm. Reached only from that arm's test,
    // which proved the receiver's class id AND keys token exactly — the same
    // proof the declared-class arm rests on, so the statically resolved body
    // is the one the dispatch tower would have found.
    let mut sub_values: Vec<(String, String)> = Vec::with_capacity(subclass_arms.len());
    for (i, arm) in subclass_arms.iter().enumerate() {
        ctx.current_block = sub_case_idxs[i];
        let value = ctx.block().call(DOUBLE, &arm.target_fn, direct_arg_slices);
        let after = ctx.block().label.clone();
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }
        sub_values.push((value, after));
    }

    ctx.current_block = fallback_idx;
    let (args_ptr, args_len) = if fallback_user_args.is_empty() {
        ("null".to_string(), "0".to_string())
    } else {
        let n = fallback_user_args.len();
        let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
        for (i, a_val) in fallback_user_args.iter().enumerate() {
            let slot = ctx
                .block()
                .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
            ctx.block().store(DOUBLE, a_val, &slot);
        }
        let ptr_reg = ctx.block().next_reg();
        ctx.block().emit_raw(format!(
            "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
            ptr_reg, n, buf_reg
        ));
        (ptr_reg, n.to_string())
    };
    if let Some(site_id) = site_id {
        crate::expr::emit_typed_feedback_record_call(
            ctx.block(),
            "js_typed_feedback_record_fallback_call",
            &[(I64, &site_id)],
        );
    }
    let method_id = crate::strings::emit_static_dispatch_id(ctx.block(), &dispatch_global);
    let fallback_value = ctx.block().call(
        DOUBLE,
        "js_native_call_method_by_id",
        &[
            (DOUBLE, recv_box),
            (I64, &method_id),
            (crate::types::PTR, &args_ptr),
            (I64, &args_len),
        ],
    );
    let after_fallback = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    let mut phi_inputs: Vec<(&str, &str)> = Vec::with_capacity(sub_values.len() + 2);
    phi_inputs.push((fast_value.as_str(), after_fast.as_str()));
    for (value, label) in &sub_values {
        phi_inputs.push((value.as_str(), label.as_str()));
    }
    phi_inputs.push((fallback_value.as_str(), after_fallback.as_str()));
    Some(ctx.block().phi(DOUBLE, &phi_inputs))
}
