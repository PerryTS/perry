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
//! skipping the call.
//!
//! NOTE (repsel Phase 3b audit, verified with `--trace llvm` + `opt -O3` on a
//! `this.field`-in-loop method): LLVM LICM does NOT hoist this check out of
//! hot loops. The volatile gate load is never hoistable (volatile ⇒
//! `!isUnordered`), and — decisively — even a plain or `atomic unordered`
//! gate load stays in the loop because the diamond's own guard-call/fallback
//! arm puts an unknown external call inside the loop body, which
//! clobber-blocks LICM for every load in the check. Per-access cost is
//! therefore paid on every iteration. The hoisted form exists as the #5093
//! versioned-loop preheader check (`emit_class_field_loop_preheader_check`,
//! sound only for call-free clone bodies), and statically-proven receivers
//! skip the diamond entirely (`collectors/ptr_shape.rs`). Do not "fix" this
//! by de-volatilizing the gate: it buys nothing (the calls still block LICM)
//! and weakens the mid-loop sticky-flip visibility guarantee for loops whose
//! bodies CAN flip the gate through a call.
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
const TYPED_LAYOUT_INTACT_BIT: &str = "4096"; // GC_OBJ_TYPED_LAYOUT_INTACT (0x1000)
const OBJ_FLAG_FROZEN_BIT: &str = "1"; // OBJ_FLAG_FROZEN (0x01)
const OBJ_FLAG_PACKED_NUMERIC_PROOF_BIT: &str = "128"; // OBJ_FLAG_PACKED_NUMERIC_PROOF (0x080)
/// `OBJ_FLAG_HAS_DESCRIPTORS | OBJ_FLAG_STABLE_TOMBSTONES`.
const OBJ_FLAG_READ_FAST_PATH_BLOCKED: &str = "3072";
/// `OBJ_FLAG_FROZEN | OBJ_FLAG_STABLE_TOMBSTONES |
/// OBJ_FLAG_HAS_DESCRIPTORS | OBJ_FLAG_PACKED_NUMERIC_PROOF` — all live in the
/// same `GcHeader::_reserved` i16, so one mask tests them.
const OBJ_FLAG_WRITE_FAST_PATH_BLOCKED: &str = "3201";
const F64_EXP_MASK: &str = "9218868437227405312"; // 0x7FF0_0000_0000_0000

/// A widening arm for the class-field shape check: one concrete subclass whose
/// instances put `property` at the SAME packed slot as the declared class does.
///
/// `keys_global` names the module global holding that subclass's canonical keys
/// array; `class_id` is its registered class id.
#[derive(Clone, Debug)]
pub(crate) struct ClassFieldSubclassArm {
    pub class_id: u32,
    pub shape_id_global: String,
}

/// A hierarchy wider than this turns the shape check into a longer compare
/// chain than the by-name fallback it replaces. Matches the dispatch-side cap
/// in `lower_call/property_get/dynamic_dispatch.rs`.
const MAX_CLASS_FIELD_SUBCLASS_ARMS: usize = 8;

/// Every transitive subclass of `class_name` that agrees with it about
/// `property`'s slot — i.e. every receiver the field fast path may accept
/// beyond the declared class itself.
///
/// ## Why this exists
///
/// `emit_class_field_inline_precheck` (and the runtime `class_field_fast_contract`
/// behind it) speculates that the receiver's dynamic class is EXACTLY the
/// expression's declared class. Inside a base class's own constructor or method
/// that bet is not merely unreliable, it is **guaranteed wrong**: `this` in
/// `Node2D`'s constructor is only ever reached through `super(...)` from a
/// subclass, so the class-id compare fails on every single store and each
/// `this.x = x` pays a full by-name `js_put_value_set`. The same holds for every
/// inherited read — a `Node2D` getter reading `this.x` misses 100% of the time.
///
/// This is the field-side counterpart of the dispatch widening in
/// `lower_call/property_get/dynamic_dispatch.rs` (#7800): one shape probe,
/// several (class id, keys) pairs.
///
/// ## Why it is sound
///
/// `class_field_global_index` lays a class out as its init chain's keyable
/// fields, root → leaf — parent fields first — so an inherited field keeps its
/// index in every subclass. That is a property of the layout algorithm, not a
/// promise, so this **re-derives the index for each candidate** and drops any
/// subclass that disagrees (a shadowing re-declaration lands at its own slot,
/// and an accessor anywhere on the chain makes `class_field_global_index`
/// return `None`). The raw-f64 candidacy of the declared type is likewise
/// re-checked per subclass: the fast path reads/writes the slot as a bare
/// double, and the per-object typed-layout intact bit only licenses that for a
/// field the *matched* class declares as a raw-f64 candidate.
pub(crate) fn class_field_subclass_arms(
    ctx: &FnCtx<'_>,
    class_name: &str,
    property: &str,
    field_index: u32,
    requires_raw_f64: bool,
) -> Vec<ClassFieldSubclassArm> {
    let Some(&declared_id) = ctx.class_ids.get(class_name) else {
        return Vec::new();
    };
    // Deterministic order: class id, then name. Codegen output must be
    // byte-reproducible (the corpus `cmp` A/B depends on it).
    let mut candidates: Vec<(&String, u32)> = ctx.class_ids.iter().map(|(k, &v)| (k, v)).collect();
    candidates.sort_unstable_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)));

    let mut arms: Vec<ClassFieldSubclassArm> = Vec::new();
    let mut seen_ids: Vec<u32> = vec![declared_id];
    for (sub_name, sub_id) in candidates {
        if sub_name == class_name || sub_id == 0 || seen_ids.contains(&sub_id) {
            continue;
        }
        if !is_transitive_subclass(ctx, sub_name, class_name) {
            continue;
        }
        // A class with computed runtime members has keys the packed layout
        // does not describe; its sets route through the by-name path anyway.
        if super::property_set::class_has_computed_runtime_members(ctx, sub_name) {
            continue;
        }
        // The layout algorithm SHOULD put an inherited field at the same index
        // in every subclass. Verify rather than assume — a shadowing
        // re-declaration or an accessor on the subclass chain breaks it.
        if crate::type_analysis::class_field_global_index(ctx, sub_name, property)
            != Some(field_index)
        {
            continue;
        }
        // The fast path's representation choice (raw double vs NaN-boxed) is
        // fixed at this site, so a subclass whose declared type disagrees would
        // have the slot read at the wrong representation.
        let sub_raw_f64 = crate::type_analysis::class_field_declared_type(ctx, sub_name, property)
            .as_ref()
            .is_some_and(crate::typed_shape::type_is_raw_f64_candidate);
        if sub_raw_f64 != requires_raw_f64 {
            continue;
        }
        let Some(keys_global) = ctx.class_keys_globals.get(sub_name).cloned() else {
            continue;
        };
        seen_ids.push(sub_id);
        arms.push(ClassFieldSubclassArm {
            class_id: sub_id,
            shape_id_global: crate::typed_shape::guard_shape_global_name_from_keys_global(
                &keys_global,
            ),
        });
        if arms.len() > MAX_CLASS_FIELD_SUBCLASS_ARMS {
            return Vec::new();
        }
    }
    arms
}

/// Does `arms` name EVERY transitive subclass of `class_name`?
///
/// When it does not — the hierarchy overflowed [`MAX_CLASS_FIELD_SUBCLASS_ARMS`]
/// (every arm is then dropped), or a subclass shadows the field, declares it
/// at another representation, or has no canonical keys global — an instance
/// of that subclass can never match the class-field guard, and every read of
/// it pays the guard AND the `js_class_field_get_ic` call behind it. In a
/// base-class method of a wide hierarchy that is every read: measured on Zod
/// 3.23 (`ZodType` has 36 subclasses), 17,400 of the 27,800 executed
/// class-field reads per 200 schema parses took that miss call.
///
/// A site whose receivers the class guard cannot name belongs on the generic
/// IC instead: its per-site word learns whichever ShapeId the site actually
/// sees and serves it with one compare, and its ways serve the next few.
/// This is the routing half of "one fast path real code actually takes".
pub(crate) fn class_field_arms_cover_every_subclass(
    ctx: &FnCtx<'_>,
    class_name: &str,
    arms: &[ClassFieldSubclassArm],
) -> bool {
    let Some(&declared_id) = ctx.class_ids.get(class_name) else {
        return true;
    };
    ctx.class_ids.iter().all(|(sub_name, &sub_id)| {
        sub_name == class_name
            || sub_id == 0
            || sub_id == declared_id
            || !is_transitive_subclass(ctx, sub_name, class_name)
            || arms.iter().any(|arm| arm.class_id == sub_id)
    })
}

/// Is `name` a transitive subclass of `ancestor`? Cycle- and depth-guarded:
/// heavily-modular packages declare same-named classes across modules, and the
/// name-keyed `ctx.classes` can then form a parent cycle (see
/// `type_analysis_class_fields.rs`).
fn is_transitive_subclass(ctx: &FnCtx<'_>, name: &str, ancestor: &str) -> bool {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut parent = ctx.classes.get(name).and_then(|c| c.extends_name.clone());
    let mut depth = 0usize;
    while let Some(p) = parent {
        depth += 1;
        if depth > 64 || !seen.insert(p.clone()) {
            return false;
        }
        if p == ancestor {
            return true;
        }
        parent = ctx.classes.get(&p).and_then(|c| c.extends_name.clone());
    }
    false
}

/// Emit the `i1` "plain finite number" predicate on a value's raw bits: true
/// iff the exponent field is not all-ones. Rejects ±Inf, every NaN (canonical
/// or boxed), and therefore every NaN-box tag — exactly the values the
/// runtime set contract (`is_plain_number_bits`) refuses to store raw.
pub(crate) fn emit_plain_finite_number_check(
    blk: &mut crate::block::LlBlock,
    value_bits: &str,
) -> String {
    let exp = blk.and(I64, value_bits, F64_EXP_MASK);
    blk.icmp_ne(I64, &exp, F64_EXP_MASK)
}

/// #5093 loop versioning: emit the whole-loop shape check in a versioned
/// loop's preheader.
///
/// This is the hoisted form of [`emit_class_field_inline_precheck`]: the same
/// strict subset of the runtime `class_field_fast_contract`, evaluated ONCE
/// before loop entry, branching to `fast_label` (the fast clone's preheader)
/// when the monomorphic shape holds and to `slow_label` (the slow clone's
/// preheader, i.e. today's guarded loop) otherwise. Evaluating it once is
/// sound only because the fast clone's body is call-free (matcher-enforced in
/// `stmt/loops.rs`): with no calls there is no allocation, so no GC can move
/// the object or run any of the runtime paths that mutate class_id /
/// keys_array / field_count / the typed-layout intact bit / the frozen bit /
/// the process-global enable flag mid-loop.
///
/// `require_raw_f64` adds the per-object typed-layout intact check (any raw-f64 read or write in
/// the loop); `require_not_frozen` adds the frozen-bit check (any write in the
/// loop). Per-store value checks are NOT emitted here — the fast clone's
/// stores keep their inline plain-finite check and side-exit to `slow_label`.
///
/// Returns `(obj_ptr, shape_ok)`: the SSA name of the receiver object pointer
/// (`inttoptr` of `obj_handle`) and the accumulated `i1` shape predicate,
/// both emitted in the deref block. The deref block is deliberately left
/// UNTERMINATED with `ctx.current_block` pointing at it: the caller lowers
/// the fast clone first, verifies it really came out call-free
/// (`LlBlock::contains_gc_unsafe_call`), and only then terminates the deref
/// block — `cond_br(shape_ok, fast, slow)` on success, or an unconditional
/// branch to the slow clone if some unpredicted lowering path emitted a call
/// (never enter a fast clone whose call-freeness is unproven). The deref
/// block dominates the fast preheader, so the fast clone may use `obj_ptr`
/// directly for raw slot access.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_class_field_loop_preheader_check(
    ctx: &mut FnCtx,
    obj_bits: &str,
    obj_handle: &str,
    expected_class_id: &str,
    expected_shape_id: &str,
    require_raw_f64: bool,
    require_not_frozen: bool,
    slow_label: &str,
) -> (String, String) {
    let deref_idx = ctx.new_block("class_field_loop.preheader.deref");
    let deref_label = ctx.block_label(deref_idx);

    // Gate: enable flag first (volatile — the runtime flips it sticky 0 -> 1
    // when descriptors / typed feedback / verify mode come into use), then
    // prove the receiver is a real heap object before dereferencing.
    {
        let blk = ctx.block();
        let flag = blk.load_volatile(I8, "@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED");
        let flag_ok = blk.icmp_eq(I8, &flag, "0");
        let tag = blk.lshr(I64, obj_bits, "48");
        let is_ptr = blk.icmp_eq(I64, &tag, POINTER_TAG_HI16);
        let above_band = blk.icmp_ugt(I64, obj_handle, HANDLE_BAND_TOP);
        let ptr_safe = blk.and(I1, &is_ptr, &above_band);
        let can_inline = blk.and(I1, &ptr_safe, &flag_ok);
        blk.cond_br(&can_inline, &deref_label, slow_label);
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

        // ObjectHeader: class_id @0 and authoritative ShapeId @4 (#8113 — the
        // two leading offsets moved down 4 when `object_type` was deleted).
        // Matching the immutable descriptor proves the live-slot bound and key
        // order.
        let cid_ptr = blk.gep(I8, &obj_ptr, &[(I64, "0")]);
        let class_id = blk.load(I32, &cid_ptr);
        let cid_ok = blk.icmp_eq(I32, &class_id, expected_class_id);

        let sid_ptr = blk.gep(I8, &obj_ptr, &[(I64, "4")]);
        let shape_id = blk.load(I32, &sid_ptr);
        let shape_ok = blk.icmp_eq(I32, &shape_id, expected_shape_id);

        let mut acc = blk.and(I1, &gtype_ok, &not_fwd);
        acc = blk.and(I1, &acc, &cid_ok);
        acc = blk.and(I1, &acc, &shape_ok);

        // #5654: a receiver that has ever had a property / accessor descriptor
        // installed on it needs the guard's descriptor-aware dispatch (an
        // accessor must fire on reads, a non-writable slot must reject
        // stores). Instance-level installs no longer flip the process-global
        // gate, so the hoisted check must vet the per-object flag — once, for
        // the whole loop: installing a descriptor mid-loop would require a
        // runtime call, which the call-free fast clone cannot make.
        let blocked = blk.and(I16, &reserved, OBJ_FLAG_READ_FAST_PATH_BLOCKED);
        let unblocked = blk.icmp_eq(I16, &blocked, "0");
        acc = blk.and(I1, &acc, &unblocked);

        if require_raw_f64 {
            let intact = blk.and(I16, &reserved, TYPED_LAYOUT_INTACT_BIT);
            let intact_ok = blk.icmp_ne(I16, &intact, "0");
            acc = blk.and(I1, &acc, &intact_ok);
        }

        if require_not_frozen {
            let blocked = blk.and(I16, &reserved, OBJ_FLAG_WRITE_FAST_PATH_BLOCKED);
            let write_fast_path_ok = blk.icmp_eq(I16, &blocked, "0");
            acc = blk.and(I1, &acc, &write_fast_path_ok);
        }

        // No terminator: the caller branches after verifying the fast clone.
        (obj_ptr, acc)
    }
}

/// #7142: the inline shape re-check that licenses routing a class-id dispatch
/// tower case to a proven-receiver method clone.
///
/// ## What the caller has already established
///
/// The caller emits this INTO a dispatch-tower case block, i.e. a block reached
/// only when `js_object_get_class_id(obj_handle)` returned a specific non-zero
/// user class id. That call already rejects the handle band, the built-in
/// Set/Map/RegExp registries, out-of-heap-range addresses, and any allocation
/// whose `GcHeader.obj_type` is not `GC_TYPE_OBJECT`
/// (`object/field_get_set/field_ops.rs`). So every predicate
/// [`emit_class_field_inline_precheck`] evaluates *before* its dereference is
/// already discharged, and the loads below need no gate/deref split — they all
/// fit in the caller's single basic block.
///
/// ## What is left, and why each one
///
/// * **ShapeId identity** — the load-bearing one. `delete inst.f` compacts
///   the packed inline slots while PRESERVING `class_id`, so a class-id match
///   alone does not prove the layout: on `class C { a; b; c }`, `delete inst.b`
///   moves `c` from slot 2 to slot 1. The compaction publishes a semantic
///   successor descriptor, so a ShapeId compare against the class's
///   `@perry_class_shape_id_*` global catches it. The check is deliberately
///   DYNAMIC: the `delete` shape barrier that stands the analysis down is
///   module-scoped while receivers alias across modules (#7143), so no static
///   proof is available at this site.
/// * **The sticky `@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED` latch** — flipped
///   the moment a descriptor / accessor lands on a class prototype or on
///   `Object.prototype`, or typed-feedback tracing turns on. This is the very
///   latch the per-access inline guard *inside the body being replaced* reads,
///   so a routed call is never weaker than the lowering it displaces.
/// * **Per-object `OBJ_FLAG_HAS_DESCRIPTORS`** — instance-level descriptor
///   installs deliberately do NOT flip the process-global latch (#5654), so
///   they are vetted per receiver, exactly as the per-access check does.
/// * **`OBJ_FLAG_FROZEN`** — a proven-receiver clone may contain field WRITES,
///   and a guard-free raw store into a frozen receiver would silently succeed
///   where the spec requires a strict-mode `TypeError`. The clone's own
///   admission rules this out only through a MODULE-scoped freeze-barrier kill,
///   so the receiver is vetted here as well.
/// * **Not-forwarded**, **`GC_TYPE_OBJECT`**, and **not a class object** — the
///   header predicates `js_object_get_class_id` does not itself check.
///
/// Cost: one volatile `i8` load of the latch, three loads off the receiver (two
/// of them from the `GcHeader` word the tower's class-id read already pulled
/// in), nine ALU ops and one conditional branch. `expected_shape_id` is expected to
/// come from an entry-hoisted slot (`LlFunction::entry_init_load_global`), so
/// the global itself is read once per function, not per call.
///
/// Emits into the CURRENT block and terminates it; the caller supplies both
/// successor labels and sets `ctx.current_block` afterwards.
pub(crate) fn emit_proven_shape_recheck(
    ctx: &mut FnCtx,
    obj_handle: &str,
    expected_shape_id: &str,
    proven_label: &str,
    generic_label: &str,
) {
    let blk = ctx.block();

    // Policy latch first — volatile for the same reason the per-access check
    // loads it volatile: the runtime flips it sticky 0 -> 1 mid-execution and
    // LLVM must not hoist a stale 0 across the flip.
    let flag = blk.load_volatile(I8, "@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED");
    let flag_ok = blk.icmp_eq(I8, &flag, "0");

    let obj_ptr = blk.inttoptr(I64, obj_handle);

    // GcHeader (precedes the object by 8 bytes): gc_flags @-7 (i8),
    // _reserved @-6 (i16).
    let gflags_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-7")]);
    let gflags = blk.load(I8, &gflags_ptr);
    let fwd = blk.and(I8, &gflags, GC_FLAG_FORWARDED_I8);
    let not_fwd = blk.icmp_eq(I8, &fwd, "0");

    let res_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-6")]);
    let reserved = blk.load(I16, &res_ptr);
    let latched = blk.and(I16, &reserved, OBJ_FLAG_WRITE_FAST_PATH_BLOCKED);
    let unlatched = blk.icmp_eq(I16, &latched, "0");

    // `class_id` @0 was already matched by the tower. ShapeId @4 proves the
    // exact immutable layout and receiver-kind descriptor (#8113 offsets).
    let sid_ptr = blk.gep(I8, &obj_ptr, &[(I64, "4")]);
    let shape_id = blk.load(I32, &sid_ptr);
    let shape_ok = blk.icmp_eq(I32, &shape_id, expected_shape_id);

    let mut acc = blk.and(I1, &flag_ok, &not_fwd);
    acc = blk.and(I1, &acc, &unlatched);
    acc = blk.and(I1, &acc, &shape_ok);
    blk.cond_br(&acc, proven_label, generic_label);
}

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
///
/// `subclass_arms` widens the shape test from "is exactly the declared class"
/// to "is the declared class or one of these subclasses, each of which puts
/// this property at this same slot" — see [`class_field_subclass_arms`] for why
/// the narrow form misses 100% of the time in a base-class body. Pass an empty
/// slice to keep the single-pair check; a class with no subclasses emits
/// byte-identical IR either way.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_class_field_inline_precheck(
    ctx: &mut FnCtx,
    obj_bits: &str,
    obj_handle: &str,
    expected_class_id: &str,
    require_raw_f64: bool,
    set_value_bits: Option<&str>,
    fast_label: &str,
    subclass_arms: &[ClassFieldSubclassArm],
    keys_global_name: &str,
) -> String {
    let guard_shape_global =
        crate::typed_shape::guard_shape_global_name_from_keys_global(keys_global_name);
    let deref_idx = ctx.new_block("class_field_inline.deref");
    let guardcall_idx = ctx.new_block("class_field_inline.guardcall");
    let deref_label = ctx.block_label(deref_idx);
    let guardcall_label = ctx.block_label(guardcall_idx);

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
        let tag = blk.lshr(I64, obj_bits, "48");
        let is_ptr = blk.icmp_eq(I64, &tag, POINTER_TAG_HI16);
        let above_band = blk.icmp_ugt(I64, obj_handle, HANDLE_BAND_TOP);
        let ptr_safe = blk.and(I1, &is_ptr, &above_band);
        blk.cond_br(&ptr_safe, &deref_label, &guardcall_label);
    }

    ctx.current_block = deref_idx;
    {
        let blk = ctx.block();
        let obj_ptr = blk.inttoptr(I64, obj_handle);

        // Two loads and two compares, not five of each. The GcHeader's first
        // 32 bits (it precedes the object by 8 bytes) are obj_type @-8,
        // gc_flags @-7 and _reserved @-6, little-endian, so every header
        // predicate is one masked compare against one expected word:
        //
        // * obj_type == GC_TYPE_OBJECT (the whole type byte);
        // * !GC_FLAG_FORWARDED;
        // * #5654: no property/accessor descriptor was ever installed on this
        //   receiver (OBJ_FLAG_READ_FAST_PATH_BLOCKED) — an accessor must fire,
        //   a non-writable slot must reject the store. The per-object flag lets
        //   the process-global gate above stay open for such installs (only
        //   prototype-level descriptors flip it);
        // * raw-f64 slots: the per-object typed layout is still INTACT (no
        //   downgrade to a NaN-boxed value);
        // * stores: not frozen (frozen objects route through the boxed
        //   setter), and #8690 no Array-subclass numeric-prefix proof the
        //   inline write could overlap.
        let mut header_mask: u32 = 0xFF | (0x80 << 8) | (3072 << 16);
        let mut header_expected: u32 = 2; // GC_TYPE_OBJECT
        if require_raw_f64 {
            header_mask |= 0x1000 << 16;
            header_expected |= 0x1000 << 16;
        }
        if set_value_bits.is_some() {
            header_mask |= (0x01 | 0x80) << 16;
        }
        debug_assert_eq!(GC_TYPE_OBJECT, "2");
        debug_assert_eq!(GC_FLAG_FORWARDED_I8, "-128");
        debug_assert_eq!(OBJ_FLAG_READ_FAST_PATH_BLOCKED, "3072");
        debug_assert_eq!(TYPED_LAYOUT_INTACT_BIT, "4096");
        debug_assert_eq!(OBJ_FLAG_FROZEN_BIT, "1");
        debug_assert_eq!(OBJ_FLAG_PACKED_NUMERIC_PROOF_BIT, "128");
        let header_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-8")]);
        let header = blk.load(I32, &header_ptr);
        let header_bits = blk.and(I32, &header, &(header_mask as i32).to_string());
        let mut acc = blk.icmp_eq(I32, &header_bits, &(header_expected as i32).to_string());

        // ObjectHeader word 0 is class_id @0 and the authoritative ShapeId @4
        // (#8113): one 64-bit compare against `(shape << 32) | class_id`.
        let identity = blk.load(I64, &obj_ptr);
        // The displaced latch's authority lives here now: this expectation is
        // what `disable_class_field_inline_guard` poisons, so the compare the
        // guard already had to make now also answers "is the inline path still
        // open?". VOLATILE for exactly the reason the latch load was — the
        // runtime flips it mid-execution and a cached expectation would take a
        // fast path the process has closed.
        let live_shape = blk.load_volatile(I32, &format!("@{guard_shape_global}"));
        let declared = expected_class_identity(blk, expected_class_id, &live_shape);
        let mut shape_ok = blk.icmp_eq(I64, &identity, &declared);
        // The declared class's own (class id, ShapeId) pair, OR any subclass
        // arm's. Each arm is a full pair — matching a class id without its
        // canonical descriptor would accept a diverged layout.
        for arm in subclass_arms {
            let arm_shape = blk.load_volatile(I32, &format!("@{}", arm.shape_id_global));
            let arm_expected = expected_class_identity(blk, &arm.class_id.to_string(), &arm_shape);
            let arm_ok = blk.icmp_eq(I64, &identity, &arm_expected);
            shape_ok = blk.or(I1, &shape_ok, &arm_ok);
        }
        acc = blk.and(I1, &acc, &shape_ok);

        if let (Some(value_bits), true) = (set_value_bits, require_raw_f64) {
            // Only a plain finite number may be stored raw. Non-finite
            // (exponent all-ones: ±Inf/NaN — rare) and every NaN-boxed tag
            // share the all-ones exponent, so a single mask/compare both keeps
            // the fast path correct and routes the boxed/downgrade cases to the
            // guard call.
            let exp = blk.and(I64, value_bits, F64_EXP_MASK);
            let finite = blk.icmp_ne(I64, &exp, F64_EXP_MASK);
            acc = blk.and(I1, &acc, &finite);
        }

        blk.cond_br(&acc, fast_label, &guardcall_label);
    }

    ctx.current_block = guardcall_idx;
    guardcall_label
}

/// The receiver test of the class-field READ guard, as ONE unsigned range
/// check: `bits - (POINTER_TAG | 0x10_0000) < 2^48 - 0x10_0000`.
///
/// Subtracting the constant maps exactly the POINTER-tagged values whose
/// 48-bit payload is above the handle band (`> HANDLE_BAND_TOP`) onto
/// `[0, 2^48 - 0x10_0000)` and every other bit pattern (other tags, a POINTER
/// tag with a small native-registry handle, plain doubles) above it, so the
/// one compare is the conjunction the flat predicate used to spell as two
/// compares, two `setcc`s and a `test`. The same subtraction yields the
/// handle (`t + 0x10_0000`), which isel folds into the displacements of the
/// loads that follow instead of re-masking the NaN-box.
const READ_RECEIVER_BIAS: u64 = crate::nanbox::POINTER_TAG | (HANDLE_BAND_TOP_U64 + 1);
const READ_RECEIVER_SPAN: u64 = (1u64 << 48) - (HANDLE_BAND_TOP_U64 + 1);
const HANDLE_BAND_TOP_U64: u64 = 0x0F_FFFF;
const _: () = assert!(READ_RECEIVER_BIAS == 0x7FFD_0000_0010_0000);
const _: () = assert!(READ_RECEIVER_SPAN == 0x0000_FFFF_FFF0_0000);

/// Emit the class-field READ guard: receiver range check, ONE ShapeId compare
/// against the poisonable per-class expectation, and — for a raw-f64 site
/// only — the class id and the per-object typed-layout intact bit.
///
/// Hit path on x86-64 (`class P { a; getA() { return this.a } }`): a boxed
/// field is `lea`/`shr`/`cmp`/`jae` (range), `mov` expectation, `cmp` against
/// `+4`, `jne`, load; a raw-f64 field adds the class id to the compare
/// (`shl`/`or` building the 64-bit word) and a one-byte `testb`/`je` of the
/// intact bit. The pre-split guard spent 11 instructions on a flat
/// tag+handle predicate and 4 on a GcHeader word mask before its compare.
///
/// This is the read-side form of [`emit_class_field_inline_precheck`], and it
/// is deliberately a separate function: the WRITE guard keeps the full header
/// word test, because `OBJ_FLAG_FROZEN` and `OBJ_FLAG_PACKED_NUMERIC_PROOF` are
/// per-object facts no ShapeId carries.
///
/// ## What a matching ShapeId already proves (the checks this guard dropped)
///
/// The generic read IC (`property_get/generic_dispatch.rs`, "No GC-header
/// load, no descriptor-flag test") dropped the same header predicates on the
/// same arguments; each is held by runtime tests in
/// `perry-runtime/src/object/shape_rules_tests.rs` / `shape_rule3.rs`:
///
/// * **`obj_type == GC_TYPE_OBJECT`** — rule 3 (#10828): no POINTER-tagged
///   non-object cell holds a value in the ShapeId range at payload `+4`. The
///   expectation is a ShapeId or the poison `u32::MAX`, and no `+4` word of
///   any kind equals either, so a match proves an ordinary object.
/// * **not `GC_FLAG_FORWARDED`** — `set_forwarding_address` (`gc/types.rs`)
///   overwrites payload `+0..8` with the new address, so a forwarded cell's
///   `+4` word is the high half of a heap address, `<= 0xFFFF` under
///   `MAX_HEAP_ADDR_EXCLUSIVE` (the `StructurallySmall` argument of rule 3),
///   far below the ShapeId floor.
/// * **no `OBJ_FLAG_HAS_DESCRIPTORS`** — rule 1 (#10824): every descriptor
///   install / removal / clear on a shaped ordinary object goes through
///   `note_descriptor_target_keyed`, which transitions the shape onto a
///   semantic successor lineage. The class ShapeId is the one module init
///   minted for the class's canonical key list and stamped on every instance
///   at birth, so a receiver still carrying it has had no descriptor change.
/// * **no `OBJ_FLAG_STABLE_TOMBSTONES`** — #10826: every successful `delete`
///   is a shape transition, so a receiver at the class ShapeId holds no hole
///   in any slot of that shape.
/// * **unstamped receivers** — rule 2: nothing but the shape allocator mints
///   into the ShapeId range, so a `parent_class_id` at `+4` cannot match.
/// * **the slot** — the expectation is the ShapeId of the class's canonical
///   keys array, whose key order IS `class_field_global_index`'s layout, so a
///   match proves `property` is an own data property at `field_index` —
///   whatever the receiver's class id. A boxed read therefore needs nothing
///   else: it returns the slot as a JS value, exactly what the generic IC
///   returns for the same ShapeId and slot.
///
/// ## What it does NOT prove (the checks this guard keeps)
///
/// * **The receiver range check** — nothing may be dereferenced before it.
/// * **The poison** — `disable_class_field_inline_guard` writes `u32::MAX`
///   into the expectation; the compare against it is the latch.
/// * **raw-f64 sites: the class id.** A class whose layout has no pointer
///   slot mints its ShapeId from the key list alone
///   (`js_object_shape_id_for_keys`, `codegen/string_pool.rs`), so an object
///   literal or another class with the same keys in the same order shares
///   it — and may hold a string where this class declares `number`. Only the
///   class id tells the raw-f64 read that the declared type is the one that
///   applies. Measured on the 2.5 base: `class SamePt { x: number; y:
///   number }`, `class BoolPt { x: boolean; y: number }` and `class SubPt
///   extends Pt {}` instances all carry `Pt`'s ShapeId, each with its own
///   class id (scenario 1 of
///   `test-files/test_gap_class_field_read_guard_shape_authority.ts`).
/// * **raw-f64 sites: `GC_OBJ_TYPED_LAYOUT_INTACT`.** A store that
///   contradicts the typed descriptor (`gc/layout.rs` `SlotVerdict::
///   Downgrade`) clears this per-object bit WITHOUT a shape transition, so
///   the ShapeId says nothing about whether the slot still holds a raw
///   double. Measured: a downgraded `Pt` keeps its ShapeId with the bit
///   clear (scenario 2 of the same fixture).
///
/// On success the IR branches to `fast_label`; on any miss to a fresh
/// `class_field_inline.guardcall` block, which is left current (the caller
/// emits the unchanged miss call there). Returns `(guardcall_label,
/// obj_handle)`: the handle is derived from the range check's subtraction and
/// is the one the fast block must address the slot through.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_class_field_read_precheck(
    ctx: &mut FnCtx,
    obj_bits: &str,
    expected_class_id: &str,
    require_raw_f64: bool,
    fast_label: &str,
    subclass_arms: &[ClassFieldSubclassArm],
    keys_global_name: &str,
) -> (String, String) {
    let guard_shape_global =
        crate::typed_shape::guard_shape_global_name_from_keys_global(keys_global_name);
    let deref_idx = ctx.new_block("class_field_inline.deref");
    let guardcall_idx = ctx.new_block("class_field_inline.guardcall");
    let deref_label = ctx.block_label(deref_idx);
    let guardcall_label = ctx.block_label(guardcall_idx);

    let obj_handle = {
        let blk = ctx.block();
        let biased = blk.sub(I64, obj_bits, &(READ_RECEIVER_BIAS as i64).to_string());
        let in_range = blk.icmp_ult(I64, &biased, &(READ_RECEIVER_SPAN as i64).to_string());
        let handle = blk.add(I64, &biased, &(HANDLE_BAND_TOP_U64 + 1).to_string());
        blk.cond_br(&in_range, &deref_label, &guardcall_label);
        handle
    };

    ctx.current_block = deref_idx;
    {
        let blk = ctx.block();
        let obj_ptr = blk.inttoptr(I64, &obj_handle);
        // The expectation is read VOLATILE: the runtime poisons it
        // mid-execution, and a cached copy would keep a closed fast path open.
        let live_shape = blk.load_volatile(I32, &format!("@{guard_shape_global}"));
        let mut ok = if require_raw_f64 {
            // ObjectHeader word 0 is class_id @0 and the ShapeId @4 (#8113):
            // one 64-bit compare against `(shape << 32) | class_id`.
            let identity = blk.load(I64, &obj_ptr);
            let declared = expected_class_identity(blk, expected_class_id, &live_shape);
            blk.icmp_eq(I64, &identity, &declared)
        } else {
            let sid_ptr = blk.gep(I8, &obj_ptr, &[(I64, "4")]);
            let shape_id = blk.load(I32, &sid_ptr);
            let mut ok = blk.icmp_eq(I32, &shape_id, &live_shape);
            for arm in subclass_arms {
                let arm_shape = blk.load_volatile(I32, &format!("@{}", arm.shape_id_global));
                let arm_ok = blk.icmp_eq(I32, &shape_id, &arm_shape);
                ok = blk.or(I1, &ok, &arm_ok);
            }
            ok
        };
        if require_raw_f64 && !subclass_arms.is_empty() {
            // Each arm is a full (class id, ShapeId) pair: the class id is
            // what licenses the raw-f64 representation (see above).
            let identity = blk.load(I64, &obj_ptr);
            for arm in subclass_arms {
                let arm_shape = blk.load_volatile(I32, &format!("@{}", arm.shape_id_global));
                let arm_expected =
                    expected_class_identity(blk, &arm.class_id.to_string(), &arm_shape);
                let arm_ok = blk.icmp_eq(I64, &identity, &arm_expected);
                ok = blk.or(I1, &ok, &arm_ok);
            }
        }
        if require_raw_f64 {
            // GcHeader `_reserved` (u16 @-6): the per-object typed-layout
            // intact bit. A native-endian half-word, like every other
            // `_reserved` reader. Same block as the identity compare, so the
            // conjunction lowers to compare-and-branches that fall through
            // into the slot load (x86-64: `movzwl`/`and`/`je` after the
            // identity `cmp`/`jne`).
            let res_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-6")]);
            let reserved = blk.load(I16, &res_ptr);
            let intact = blk.and(I16, &reserved, TYPED_LAYOUT_INTACT_BIT);
            let intact_ok = blk.icmp_ne(I16, &intact, "0");
            ok = blk.and(I1, &ok, &intact_ok);
        }
        blk.cond_br(&ok, fast_label, &guardcall_label);
    }

    ctx.current_block = guardcall_idx;
    (guardcall_label, obj_handle)
}

/// `(shape_id << 32) | class_id` — the little-endian value of an
/// `ObjectHeader`'s first word for an instance of that exact class and layout.
fn expected_class_identity(
    blk: &mut crate::block::LlBlock,
    class_id: &str,
    shape_id: &str,
) -> String {
    let class_bits = blk.zext(I32, class_id, I64);
    let shape_bits = blk.zext(I32, shape_id, I64);
    let shape_high = blk.shl(I64, &shape_bits, "32");
    blk.or(I64, &shape_high, &class_bits)
}

#[cfg(test)]
mod read_receiver_range_tests {
    use super::{HANDLE_BAND_TOP_U64, READ_RECEIVER_BIAS, READ_RECEIVER_SPAN};

    /// The predicate the flat form spelled out: POINTER tag AND a payload
    /// above the native-registry handle band.
    fn flat(bits: u64) -> bool {
        (bits >> 48) == 0x7FFD && (bits & 0x0000_FFFF_FFFF_FFFF) > HANDLE_BAND_TOP_U64
    }

    /// The emitted form: one biased unsigned compare.
    fn biased(bits: u64) -> bool {
        bits.wrapping_sub(READ_RECEIVER_BIAS) < READ_RECEIVER_SPAN
    }

    /// The range check is EXACTLY the flat predicate, including at every
    /// boundary: each neighbouring tag, the handle band's top and the first
    /// address above it, the top of the 48-bit payload, and plain doubles.
    /// A one-off in either constant flips at least one of these.
    #[test]
    fn biased_range_check_equals_tag_and_handle_predicate() {
        let payloads = [
            0u64,
            1,
            HANDLE_BAND_TOP_U64 - 1,
            HANDLE_BAND_TOP_U64,
            HANDLE_BAND_TOP_U64 + 1,
            HANDLE_BAND_TOP_U64 + 2,
            0x7F12_3456_7890,
            0x0000_FFFF_FFFF_FFFE,
            0x0000_FFFF_FFFF_FFFF,
        ];
        for tag in [
            0u64, 0x3FF0, 0x7FF8, 0x7FF9, 0x7FFA, 0x7FFC, 0x7FFD, 0x7FFE, 0x7FFF, 0xFFFF,
        ] {
            for payload in payloads {
                let bits = (tag << 48) | payload;
                assert_eq!(
                    biased(bits),
                    flat(bits),
                    "tag {tag:#x} payload {payload:#x}: biased check disagrees"
                );
            }
        }
        // The handle the fast path addresses through is the payload.
        let bits = (0x7FFDu64 << 48) | 0x7F12_3456_7890;
        assert_eq!(
            bits.wrapping_sub(READ_RECEIVER_BIAS) + HANDLE_BAND_TOP_U64 + 1,
            0x7F12_3456_7890
        );
    }
}
