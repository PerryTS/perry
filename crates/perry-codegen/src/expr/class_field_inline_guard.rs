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
const OBJECT_TYPE_REGULAR: &str = "1";
const TYPED_LAYOUT_INTACT_BIT: &str = "4096"; // GC_OBJ_TYPED_LAYOUT_INTACT (0x1000)
const OBJ_FLAG_FROZEN_BIT: &str = "1"; // OBJ_FLAG_FROZEN (0x01)
const OBJ_FLAG_HAS_DESCRIPTORS_BIT: &str = "2048"; // OBJ_FLAG_HAS_DESCRIPTORS (0x800)
/// `OBJ_FLAG_FROZEN | OBJ_FLAG_HAS_DESCRIPTORS` — both live in the same
/// `GcHeader::_reserved` i16, so one mask tests both.
const OBJ_FLAG_FROZEN_OR_DESCRIPTORS: &str = "2049";
const F64_EXP_MASK: &str = "9218868437227405312"; // 0x7FF0_0000_0000_0000

/// A widening arm for the class-field shape check: one concrete subclass whose
/// instances put `property` at the SAME packed slot as the declared class does.
///
/// `keys_global` names the module global holding that subclass's canonical keys
/// array; `class_id` is its registered class id.
#[derive(Clone, Debug)]
pub(crate) struct ClassFieldSubclassArm {
    pub class_id: u32,
    pub keys_global: String,
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
            keys_global,
        });
        if arms.len() > MAX_CLASS_FIELD_SUBCLASS_ARMS {
            return Vec::new();
        }
    }
    arms
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
/// `max_field_index` is the largest packed slot index the loop touches
/// (`field_count ugt max_field_index` covers every access). `require_raw_f64`
/// adds the per-object typed-layout intact check (any raw-f64 read or write in
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
    expected_keys: &str,
    max_field_index: u32,
    require_raw_f64: bool,
    require_not_frozen: bool,
    slow_label: &str,
) -> (String, String) {
    let deref_idx = ctx.new_block("class_field_loop.preheader.deref");
    let deref_label = ctx.block_label(deref_idx);
    let max_field_index_str = max_field_index.to_string();

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

        // ObjectHeader: object_type @0 (i32)==REGULAR, class_id @4 (i32),
        // field_count @12 (i32), keys_array @16 (i64).
        let object_type = blk.load(I32, &obj_ptr);
        let ot_ok = blk.icmp_eq(I32, &object_type, OBJECT_TYPE_REGULAR);

        let cid_ptr = blk.gep(I8, &obj_ptr, &[(I64, "4")]);
        let class_id = blk.load(I32, &cid_ptr);
        let cid_ok = blk.icmp_eq(I32, &class_id, expected_class_id);

        let fc_ptr = blk.gep(I8, &obj_ptr, &[(I64, "12")]);
        let field_count = blk.load(I32, &fc_ptr);
        let fc_ok = blk.icmp_ugt(I32, &field_count, &max_field_index_str);

        let ka_ptr = blk.gep(I8, &obj_ptr, &[(I64, "16")]);
        let keys_array = blk.load(I64, &ka_ptr);
        let ka_ok = blk.icmp_eq(I64, &keys_array, expected_keys);

        let mut acc = blk.and(I1, &gtype_ok, &not_fwd);
        acc = blk.and(I1, &acc, &ot_ok);
        acc = blk.and(I1, &acc, &cid_ok);
        acc = blk.and(I1, &acc, &fc_ok);
        acc = blk.and(I1, &acc, &ka_ok);

        // #5654: a receiver that has ever had a property / accessor descriptor
        // installed on it needs the guard's descriptor-aware dispatch (an
        // accessor must fire on reads, a non-writable slot must reject
        // stores). Instance-level installs no longer flip the process-global
        // gate, so the hoisted check must vet the per-object flag — once, for
        // the whole loop: installing a descriptor mid-loop would require a
        // runtime call, which the call-free fast clone cannot make.
        let has_desc = blk.and(I16, &reserved, OBJ_FLAG_HAS_DESCRIPTORS_BIT);
        let no_desc = blk.icmp_eq(I16, &has_desc, "0");
        acc = blk.and(I1, &acc, &no_desc);

        if require_raw_f64 {
            let intact = blk.and(I16, &reserved, TYPED_LAYOUT_INTACT_BIT);
            let intact_ok = blk.icmp_ne(I16, &intact, "0");
            acc = blk.and(I1, &acc, &intact_ok);
        }

        if require_not_frozen {
            let frozen = blk.and(I16, &reserved, OBJ_FLAG_FROZEN_BIT);
            let not_frozen = blk.icmp_eq(I16, &frozen, "0");
            acc = blk.and(I1, &acc, &not_frozen);
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
/// * **`keys_array` identity** — the load-bearing one. `delete inst.f` compacts
///   the packed inline slots while PRESERVING `class_id`, so a class-id match
///   alone does not prove the layout: on `class C { a; b; c }`, `delete inst.b`
///   moves `c` from slot 2 to slot 1. The compaction installs a freshly CLONED
///   keys array (`object/delete_rest.rs` — `js_array_alloc` +
///   `set_object_keys_array`, cloned precisely because the old array is shared
///   between every instance of the shape), so a pointer compare against the
///   class's `@perry_class_keys_*` global catches it. The check is deliberately
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
/// * **Not-forwarded** and **`object_type == OBJECT_TYPE_REGULAR`** — the two
///   header predicates `js_object_get_class_id` does not itself check.
///
/// Cost: one volatile `i8` load of the latch, three loads off the receiver (two
/// of them from the `GcHeader` word the tower's class-id read already pulled
/// in), nine ALU ops and one conditional branch. `expected_keys` is expected to
/// come from an entry-hoisted slot (`LlFunction::entry_init_load_global`), so
/// the global itself is read once per function, not per call.
///
/// Emits into the CURRENT block and terminates it; the caller supplies both
/// successor labels and sets `ctx.current_block` afterwards.
pub(crate) fn emit_proven_shape_recheck(
    ctx: &mut FnCtx,
    obj_handle: &str,
    expected_keys: &str,
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
    let latched = blk.and(I16, &reserved, OBJ_FLAG_FROZEN_OR_DESCRIPTORS);
    let unlatched = blk.icmp_eq(I16, &latched, "0");

    // ObjectHeader: object_type @0 (i32), keys_array @16 (i64). `class_id` @4
    // was already matched by the tower's `js_object_get_class_id` compare.
    let object_type = blk.load(I32, &obj_ptr);
    let ot_ok = blk.icmp_eq(I32, &object_type, OBJECT_TYPE_REGULAR);

    let ka_ptr = blk.gep(I8, &obj_ptr, &[(I64, "16")]);
    let keys_array = blk.load(I64, &ka_ptr);
    let ka_ok = blk.icmp_eq(I64, &keys_array, expected_keys);

    let mut acc = blk.and(I1, &flag_ok, &not_fwd);
    acc = blk.and(I1, &acc, &unlatched);
    acc = blk.and(I1, &acc, &ot_ok);
    acc = blk.and(I1, &acc, &ka_ok);
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
    expected_keys: &str,
    field_index: u32,
    require_raw_f64: bool,
    set_value_bits: Option<&str>,
    fast_label: &str,
    subclass_arms: &[ClassFieldSubclassArm],
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
        if subclass_arms.is_empty() {
            // Byte-for-byte the pre-widening and-chain. A class with no
            // eligible subclass must emit IDENTICAL IR, so the corpus-wide
            // `cmp` stays a usable no-regression instrument (a reordered
            // and-chain alone made 17 of 19 corpus binaries differ for no
            // behavioural reason).
            acc = blk.and(I1, &acc, &cid_ok);
            acc = blk.and(I1, &acc, &fc_ok);
            acc = blk.and(I1, &acc, &ka_ok);
        } else {
            // The declared class's own (class id, keys) pair, OR any subclass
            // arm's. Each arm is a full pair — matching a class id without its
            // canonical keys array would accept an instance that has since
            // grown a property and no longer has the packed layout this slot
            // index describes.
            let mut shape_ok = blk.and(I1, &cid_ok, &ka_ok);
            for arm in subclass_arms {
                let arm_cid_ok = blk.icmp_eq(I32, &class_id, &arm.class_id.to_string());
                let arm_keys = blk.load(I64, &format!("@{}", arm.keys_global));
                let arm_ka_ok = blk.icmp_eq(I64, &keys_array, &arm_keys);
                let arm_ok = blk.and(I1, &arm_cid_ok, &arm_ka_ok);
                shape_ok = blk.or(I1, &shape_ok, &arm_ok);
            }
            acc = blk.and(I1, &acc, &shape_ok);
            acc = blk.and(I1, &acc, &fc_ok);
        }

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
