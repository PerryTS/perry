//! #5093: codegen-inlined class shape checks, and the class-layout facts
//! behind them.
//!
//! A class-field READ or STORE is not lowered here: it is the shape site
//! (`property_get/generic_dispatch.rs`, `put_value_store_ic.rs`), whose word
//! follows the receiver's actual ShapeId (#12483). What remains are the checks
//! a typed METHOD CLONE's admission makes against its receiver class's
//! ShapeId (`lower_call/method_override.rs`, `dynamic_dispatch.rs`), and the
//! facts that say whether a class's static ShapeId describes its instances at
//! all ([`class_instances_keep_birth_shape`]) and which of its slots are `F64`
//! lanes ([`class_field_site_raw_f64`]).
//!
//! The exact ShapeId carries the raw-f64 lane rep, so a representation change
//! must move to a different ShapeId before a check against it can pass again.

use crate::types::{I1, I16, I32, I64, I8};

use super::FnCtx;

// Mirror of the runtime constants the inline check reproduces. Kept as literal
// decimals because the emitted IR is textual.
const GC_FLAG_FORWARDED_I8: &str = "-128"; // 0x80 as i8
/// `OBJ_FLAG_FROZEN | OBJ_FLAG_STABLE_TOMBSTONES |
/// OBJ_FLAG_HAS_DESCRIPTORS`. Numeric proof is a different ShapeId, so the
/// exact shape comparison below excludes it.
const OBJ_FLAG_WRITE_FAST_PATH_BLOCKED: &str = "3073";
const F64_EXP_MASK: &str = "9218868437227405312"; // 0x7FF0_0000_0000_0000

/// A widening arm for the class-field shape check: one concrete subclass whose
/// instances put `property` at the SAME packed slot as the declared class does.
///
/// `keys_global` names the module global holding that subclass's canonical keys
/// array (its guard operand comes from it, `class_shape_id_operand_on_block`);
/// `class_id` is its registered class id.
#[derive(Clone, Debug)]
pub(crate) struct ClassFieldSubclassArm {
    pub class_id: u32,
    pub keys_global: String,
    /// The subclass's birth rep word (`CrossModuleCtx::class_birth_reps`, T1).
    pub birth_rep: u64,
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
/// A check against the declared class's ShapeId (and the runtime
/// `class_field_fast_contract`) speculates that the receiver's dynamic class is EXACTLY the
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
    let mut candidates: Vec<(&str, u32)> = transitive_subclasses(ctx, class_name)
        .into_iter()
        .filter_map(|name| ctx.class_ids.get(name).map(|&id| (name, id)))
        .collect();
    candidates.sort_unstable_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)));

    let mut arms: Vec<ClassFieldSubclassArm> = Vec::new();
    let mut seen_ids: Vec<u32> = vec![declared_id];
    for (sub_name, sub_id) in candidates {
        if sub_id == 0 || seen_ids.contains(&sub_id) {
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
        let Some(keys_global) = ctx.class_keys_globals.get(sub_name).cloned() else {
            continue;
        };
        // A raw site (`class_field_site_raw_f64`) has an `F64` lane at this
        // slot in every arm's birth rep; a boxed site reads any lane boxed.
        let birth_rep = ctx.class_birth_reps.get(&keys_global).copied().unwrap_or(0);
        if requires_raw_f64 && !crate::typed_shape::birth_rep_slot_is_f64(birth_rep, field_index) {
            continue;
        }
        seen_ids.push(sub_id);
        arms.push(ClassFieldSubclassArm {
            class_id: sub_id,
            birth_rep: ctx.class_birth_reps.get(&keys_global).copied().unwrap_or(0),
            keys_global,
        });
        if arms.len() > MAX_CLASS_FIELD_SUBCLASS_ARMS {
            return Vec::new();
        }
    }
    arms
}

/// Charter step 5, P4: may a class-field site treat slot `field_index` of
/// `class_name` as a raw double? Exactly when the slot is an `F64` lane of the
/// birth rep of every ShapeId the site's guard compares against: the declared
/// class's and each subclass arm's. A subclass whose birth rep has no `F64`
/// lane at the slot makes the site boxed. The declared type is not consulted:
/// the birth rep is the one decision (T1 `class_birth_rep_in`), and the
/// runtime's IC contract flag follows the same answer.
pub(crate) fn class_field_site_raw_f64(
    ctx: &FnCtx<'_>,
    class_name: &str,
    property: &str,
    field_index: u32,
) -> bool {
    class_birth_slot_is_f64(ctx, class_name, field_index)
        && class_field_subclass_arms(ctx, class_name, property, field_index, false)
            .iter()
            .all(|arm| crate::typed_shape::birth_rep_slot_is_f64(arm.birth_rep, field_index))
}

/// Is `field_index` an `F64` lane of `class_name`'s own birth rep?
pub(crate) fn class_birth_slot_is_f64(ctx: &FnCtx<'_>, class_name: &str, field_index: u32) -> bool {
    ctx.class_keys_globals
        .get(class_name)
        .and_then(|keys_global| ctx.class_birth_reps.get(keys_global))
        .is_some_and(|&rep| crate::typed_shape::birth_rep_slot_is_f64(rep, field_index))
}

/// Do instances of `class_name` (or of any subclass) gain keys in their
/// constructors beyond the declared layout?
///
/// The class-field guards compare the receiver's ShapeId with the class's
/// BIRTH ShapeId (the canonical keys global). A constructor that stores an
/// undeclared key (`this.parse = this.parse.bind(this)`, Zod's `ZodType`)
/// moves every finished instance off that shape, so every guard compare on it
/// misses and pays the guard call and the by-name fallback. Measured on
/// Zod 3.23: every one of the 7,600 class-field store misses per 200 parses
/// was a receiver with 28 keys against a 5-key birth shape. Such a site
/// belongs on the generic store IC, whose word learns the shapes the site
/// actually sees.
///
/// A class with instance private elements, its own or an ancestor's, is the
/// same case (#11791): construction adds the class brand and each private
/// field to the instance's shape, so no finished instance carries the birth
/// ShapeId either.
pub(crate) fn class_instances_grow_past_layout(ctx: &FnCtx<'_>, class_name: &str) -> bool {
    let grows = |name: &str| {
        ctx.classes.get(name).copied().is_some_and(|class| {
            crate::lower_call::new_alloc::constructor_added_key_count(ctx, class) > 0
                || class_completes_off_guarded_shapes(ctx, name, class)
        })
    };
    grows(class_name)
        || transitive_subclasses(ctx, class_name)
            .into_iter()
            .any(grows)
}

/// Constructor-store hints reserve capacity, but their keys follow execution
/// order. Such instances cannot use guards against the declared field layout.
pub(crate) fn class_instances_have_constructor_reservations(
    ctx: &FnCtx<'_>,
    class_name: &str,
) -> bool {
    let reserves = |name: &str| {
        let mut current = ctx.classes.get(name).copied();
        for _ in 0..64 {
            let Some(class) = current else { break };
            if class
                .fields
                .iter()
                .any(|field| field.origin.is_reservation())
            {
                return true;
            }
            current = class
                .extends_name
                .as_deref()
                .and_then(|parent| ctx.classes.get(parent).copied());
        }
        false
    };
    reserves(class_name)
        || transitive_subclasses(ctx, class_name)
            .into_iter()
            .any(reserves)
}

/// Is the class's static ShapeId (its canonical keys global) the shape its
/// instances are born on AND keep once constructed? Only then can a static
/// supplier name it. A class whose keys are constructor-store reservations
/// (every plain JavaScript class) is born without them and reaches a shape by
/// transitions; a constructor that adds keys or private elements moves every
/// instance off it. Such a receiver is served by the learned site word, which
/// follows the shape the instance actually has (#12483).
pub(crate) fn class_instances_keep_birth_shape(ctx: &FnCtx<'_>, class_name: &str) -> bool {
    !class_instances_have_constructor_reservations(ctx, class_name)
        && !class_instances_grow_past_layout(ctx, class_name)
        && !class_instances_born_on_family(ctx, class_name)
}

/// An instance born on its class's static family layout (object model case
/// 2c) carries the family's layout id for life, never the class keys'
/// static ShapeId.
fn class_instances_born_on_family(ctx: &FnCtx<'_>, class_name: &str) -> bool {
    let born = |name: &str| {
        ctx.classes
            .get(name)
            .copied()
            .zip(ctx.class_ids.get(name).copied())
            .is_some_and(|(class, cid)| {
                crate::lower_call::new_alloc::class_born_on_family(ctx, class, cid)
            })
    };
    born(class_name) || transitive_subclasses(ctx, class_name).into_iter().any(born)
}

/// The ShapeId a read site of `class_name`'s field at `field_index` compares
/// its receiver against in place of its learned word: the class's static
/// ShapeId, when every receiver the site can name is born on it and keeps it
/// (`class_instances_keep_birth_shape`), no subclass exists to carry another
/// one. Nothing else about the site
/// changes: one ShapeId compare, one slot load (at the layout's index), and
/// the site's own miss, which keeps learning the word for any receiver that
/// is not on that shape. Comparing against a constant is what lets reads of
/// one receiver share the proof (`class_guard_memo`, or LLVM), which loads of
/// two learned words never can (#12483).
///
/// `this` in an object literal's method names the literal's shape class
/// (`guarded_this_class`), but nothing makes the receiver that literal: its
/// methods run on `Object.create` children and on any object they are copied
/// to, each on its own shape. Such a read keeps the learned word.
pub(crate) fn class_field_predicted_shape(
    ctx: &FnCtx<'_>,
    receiver: &perry_hir::Expr,
    class_name: &str,
    field_index: u32,
) -> Option<PredictedShape> {
    if matches!(receiver, perry_hir::Expr::This) && ctx.guarded_this_class.is_some() {
        return None;
    }
    let keys_global = ctx.class_keys_globals.get(class_name)?.clone();
    if !class_instances_keep_birth_shape(ctx, class_name)
        || !transitive_subclasses(ctx, class_name).is_empty()
        || super::property_set::class_has_computed_runtime_members(ctx, class_name)
    {
        return None;
    }
    Some(PredictedShape {
        keys_global,
        slot: field_index,
    })
}

/// See [`class_field_predicted_shape`].
#[derive(Clone, Debug)]
pub(crate) struct PredictedShape {
    /// The class's canonical keys global; its ShapeId is the static immediate
    /// when the driver assigned one, else a load of the shape global
    /// (`class_shape_id_operand_on_block`).
    pub keys_global: String,
    pub slot: u32,
}

/// Do finished instances of `class_name` (or of any subclass) carry a private
/// brand or a private field (#11791)? Then no finished instance is on the
/// birth ShapeId the class-field guards compare, even a receiver the compiler
/// proved to be the class.
pub(crate) fn class_instances_carry_private_elements(ctx: &FnCtx<'_>, class_name: &str) -> bool {
    let carries = |name: &str| {
        ctx.classes
            .get(name)
            .copied()
            .is_some_and(|class| class_completes_off_guarded_shapes(ctx, name, class))
    };
    carries(class_name)
        || transitive_subclasses(ctx, class_name)
            .into_iter()
            .any(carries)
}

/// Does constructing `class` leave every instance on a shape the class guards
/// do not accept? Construction that adds a private brand or field moves the
/// instance off its birth ShapeId; when the class has a static completed
/// private content (`codegen::static_private_class`), the guards accept the
/// shape it lands on as a compatible completed id, so it does not.
fn class_completes_off_guarded_shapes(
    ctx: &FnCtx<'_>,
    name: &str,
    class: &perry_hir::Class,
) -> bool {
    class_chain_has_private_instance_elements(ctx, class)
        && !crate::codegen::static_private_class::class_has_static_private_final(ctx, name)
}

/// Does constructing `class` add a private brand or a private field, from
/// `class` itself or any class it extends? Cycle- and depth-guarded like
/// [`transitive_subclasses`].
fn class_chain_has_private_instance_elements(ctx: &FnCtx<'_>, class: &perry_hir::Class) -> bool {
    let mut current = Some(class);
    let mut depth = 0usize;
    while let Some(c) = current {
        if c.has_private_instance_elements() {
            return true;
        }
        depth += 1;
        if depth > 64 {
            return false;
        }
        current = c
            .extends_name
            .as_deref()
            .and_then(|parent| ctx.classes.get(parent).copied());
    }
    false
}

/// Every transitive subclass of `ancestor` (never `ancestor` itself).
fn transitive_subclasses<'c>(ctx: &FnCtx<'c>, ancestor: &str) -> Vec<&'c str> {
    ctx.class_hierarchy
        .descendants(ancestor, Some(MAX_SUBCLASS_DEPTH))
}

/// How many `extends` steps a subclass may sit below the class it is checked
/// against. Heavily-modular packages declare same-named classes across
/// modules, and the name-keyed `ctx.classes` can then form a parent cycle (see
/// `type_analysis_class_fields.rs`); the search stops at a repeated name and at
/// this depth.
const MAX_SUBCLASS_DEPTH: usize = 64;

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
/// [`emit_class_field_read_precheck`] evaluates *before* its dereference is
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
/// Cost: three loads off the receiver (two
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
    let shape_ok =
        crate::typed_shape::emit_compatible_shape_eq(blk, &shape_id, expected_shape_id, &[]);

    let mut acc = not_fwd;
    acc = blk.and(I1, &acc, &unlatched);
    acc = blk.and(I1, &acc, &shape_ok);
    blk.cond_br(&acc, proven_label, generic_label);
}

/// Emit the receiver check a typed method clone's admission makes: receiver
/// range check, ONE ShapeId compare against the class's own ShapeId global,
/// and — for a raw-f64 receiver — the class id. (A class-field read is the
/// shape site, not this check: #12483.) The exact ShapeId comparison rejects
/// the Array-subclass numeric-proof sibling.
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
///   expectation is the class's ShapeId, so a match proves an ordinary object.
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
///
/// ## What it no longer consults (S6)
///
/// The expectation used to be a poisonable twin of the class ShapeId,
/// `@perry_class_guard_shape_*`, which `disable_class_field_inline_guard`
/// overwrote with `u32::MAX` — a process switch saying "do not trust the
/// shape". Its triggers were a prototype-level accessor on a declared field
/// name, typed-feedback tracing, `PERRY_VERIFY_TYPED_INTACT` and the
/// `PERRY_DISABLE_CLASS_FIELD_INLINE` knob. None of them changes the answer
/// this read gives: a class instance carries every declared field as an OWN
/// data property from birth (the canonical keys array), and an own data
/// property shadows any prototype accessor (scenario 3 of
/// `test-files/test_gap_5654_class_field_inline_descriptor.ts`; the generic
/// IC, which never had a switch, gives the same answer for the same ShapeId
/// and slot). The other three are diagnostics that only decided whether the
/// guard CALL observed the access.
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
/// * **raw-f64 sites: exact ShapeId birth rep.** The compared id carries
///   the F64 lane fact; a contradictory store moves to a sibling id.
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
    let deref_idx = ctx.new_block("class_field_inline.deref");
    let guardcall_idx = ctx.new_block("class_field_inline.guardcall");
    let deref_label = ctx.block_label(deref_idx);
    let guardcall_label = ctx.block_label(guardcall_idx);

    // The receiver test, as ONE unsigned range check
    // (`crate::expr::receiver_range`): POINTER tag and a payload above the
    // native-handle band. The same subtraction yields the handle
    // (`biased + 0x10_0000`), which isel folds into the displacements of the
    // loads that follow instead of re-masking the NaN-box.
    let obj_handle = {
        let blk = ctx.block();
        let recv = crate::expr::receiver_range::emit_fused_receiver_test(blk, obj_bits);
        let handle = crate::expr::receiver_range::emit_handle(blk, &recv.biased);
        blk.cond_br(&recv.is_object_pointer, &deref_label, &guardcall_label);
        handle
    };

    ctx.current_block = deref_idx;
    {
        let blk = ctx.block();
        crate::expr::receiver_range::emit_route_note(
            blk,
            crate::expr::receiver_range::Route::ClassRead,
        );
        let obj_ptr = blk.inttoptr(I64, &obj_handle);
        // The expectation is the class's own ShapeId (see the write guard
        // above: the static immediate, else a volatile load of the global).
        let live_shape =
            crate::typed_shape::class_shape_id_operand_on_block(blk, keys_global_name, true);
        let mut ok = if require_raw_f64 {
            // ObjectHeader word 0 is class_id @0 and the ShapeId @4 (#8113):
            // one 64-bit compare against `(shape << 32) | class_id`.
            let identity = blk.load(I64, &obj_ptr);
            let declared = expected_class_identity(blk, expected_class_id, &live_shape);
            crate::typed_shape::emit_compatible_class_shape_eq(
                blk,
                &identity,
                expected_class_id,
                &live_shape,
                &declared,
                &[],
            )
        } else {
            let sid_ptr = blk.gep(I8, &obj_ptr, &[(I64, "4")]);
            let shape_id = blk.load(I32, &sid_ptr);
            let mut ok =
                crate::typed_shape::emit_compatible_shape_eq(blk, &shape_id, &live_shape, &[]);
            for arm in subclass_arms {
                let arm_shape = crate::typed_shape::class_shape_id_operand_on_block(
                    blk,
                    &arm.keys_global,
                    true,
                );
                let arm_ok =
                    crate::typed_shape::emit_compatible_shape_eq(blk, &shape_id, &arm_shape, &[]);
                ok = blk.or(I1, &ok, &arm_ok);
            }
            ok
        };
        if require_raw_f64 && !subclass_arms.is_empty() {
            // Each arm is a full (class id, ShapeId) pair: the class id is
            // what licenses the raw-f64 representation (see above).
            let identity = blk.load(I64, &obj_ptr);
            for arm in subclass_arms {
                let arm_shape = crate::typed_shape::class_shape_id_operand_on_block(
                    blk,
                    &arm.keys_global,
                    true,
                );
                let arm_expected =
                    expected_class_identity(blk, &arm.class_id.to_string(), &arm_shape);
                let arm_ok = crate::typed_shape::emit_compatible_class_shape_eq(
                    blk,
                    &identity,
                    &arm.class_id.to_string(),
                    &arm_shape,
                    &arm_expected,
                    &[],
                );
                ok = blk.or(I1, &ok, &arm_ok);
            }
        }
        // Charter step 5, P4: no per-object typed-layout bit. A raw-f64 read
        // is emitted only for an `F64` lane of every compared id's birth rep
        // (`class_field_site_raw_f64`), so the identity compare alone proves
        // the slot holds a Number.
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
