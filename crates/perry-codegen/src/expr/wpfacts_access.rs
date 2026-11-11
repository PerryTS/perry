//! wpfacts slice 2: the zero-guard tier (design §5.1), the consumer of the
//! whole-program facts at a field access.
//!
//! A field read, write or update `e.f` whose receiver `e` is a binding (a
//! local, or `this`) that the facts prove holds only instances of
//! prefix-stable classes (F3, F4), where `f` is a definite data birth field in
//! every one of them (F1', F3), lowers to a bare fixed-offset load/store (§3.7
//! case 1, every class has `f` at one slot), or, when the classes disagree on
//! the slot, to a select over their class ids, each arm a fixed offset (§3.7
//! case 2; the ids are constants of the compile, nothing is learned). No
//! receiver test (unless the binding can also hold null/undefined: then the
//! site keeps that one test, and its miss arm is the complete site operation,
//! which throws), no ShapeId compare, no miss call.
//!
//! The receiver facts come from the whole-program solve (`locals`,
//! `this_of`, and, for a receiver that is itself a field read, the field's
//! holder facts `FactsField::classes`).
//! The GC write barrier stays for stores that can carry a pointer (F2 decides
//! which can); the raw-f64 lane discipline stays for stores into an F64 lane.
//!
//! Decision 150: a proof is GUARDED when the binding can also hold values
//! outside its class set (`FactsRecv::checked`: ⊤ from interpreted or
//! unmodeled code, other objects, primitives), or when the program can run
//! interpreted code at all (`ElisionFacts::interp_guard`). A guarded read
//! tests what the proof needs at the site (the runtime's
//! `PERRY_INTERP_PROTO_WRITTEN` word; the receiver test; the class id
//! against the set) and its miss arm is the site's complete generic
//! operation, lowered without the facts; it rejoins the hit arm. Guarded
//! proofs serve reads only; stores, updates, number-context raw reads and
//! the F6 element proof stay on their checked routes.
//!
//! Every proof is cross-checked against codegen's own layout
//! (`class_field_global_index`): a disagreement means no elision.
//!
//! Every elision records its proof: with `PERRY_FACTS_DUMP` set, one line per
//! elided site goes to `<dump>.elisions`; in a `PERRY_CHECK_CONTRACTS=1` or
//! `PERRY_VERIFY_FACTS=1` build (`verify_facts`) each elided site calls
//! `perry_wpfacts_check_field` (perry-runtime `wpfacts_check.rs`), which
//! re-derives class, slot and kind from the object and aborts, naming the
//! site, on a mismatch.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::POINTER_MASK_I64;
use crate::native_value::{BoundsState, BufferAccessMode, LoweredValue, NativeRep, SemanticKind};
use crate::types::{DOUBLE, I32, I64, I8};

use super::{lower_expr, FnCtx};

/// `perry_wpfacts_check_field` flag bits (perry-runtime `wpfacts_check.rs`).
pub(crate) const WPF_STORE: u32 = 1;
pub(crate) const WPF_NULLABLE: u32 = 2;
pub(crate) const WPF_RAW_F64: u32 = 4;

/// Kind-mask bits (`perry_wpfacts::KindFact::mask`).
const K_NUM: u16 = 1;
const K_STR: u16 = 2;
const K_OBJ: u16 = 128;
/// Kinds whose values can carry a heap pointer: string, bigint, symbol,
/// object.
const K_POINTER: u16 = 2 | 32 | 64 | 128;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    Get,
    Set,
    Update,
}

/// What one elided site relies on.
#[derive(Clone, Debug)]
pub(crate) struct FieldProof {
    pub class_ids: Vec<u32>,
    pub class_names: Vec<String>,
    /// The slot of the first class; [`FieldProof::slots`] has every class's.
    pub slot: u32,
    /// The slot per class, aligned with `class_ids` (case 2 when they differ).
    pub slots: Vec<u32>,
    /// The binding can also hold null/undefined: the receiver test stays.
    pub nullable: bool,
    /// F2: the union of the field's kinds over the class set.
    pub kind_mask: u16,
    /// F6 over the whole set.
    pub ta_stable: bool,
    /// F6 element kind, the same for the whole set.
    pub ta_kind: Option<u8>,
    /// Every class's birth lane at the slot is `F64` and the field only ever
    /// holds Numbers: the slot always holds a canonical double.
    pub raw_f64: bool,
    /// The field only ever holds Numbers (F2 is exactly Number), the read is
    /// guard-free and the receiver is a heap object: the read is a Number,
    /// and the slot's word is that Number's raw double (a Number has one
    /// encoding), on a boxed slot as on an `F64` lane.
    pub number: bool,
    /// Some class's birth lane at the slot is `F64`: a store keeps the
    /// raw-slot discipline.
    pub any_f64_lane: bool,
    /// The binding the proof is about (`local 12`, `this of fn 7`).
    pub binding: String,
    /// The binding can hold values outside `class_ids`: the site compares
    /// the receiver's class id (decision 150, 5b case 2).
    pub checked: bool,
    /// Interpreted code can run: the site tests
    /// `PERRY_INTERP_PROTO_WRITTEN` (decision 150).
    pub interp_guard: bool,
    /// The key is declared or reserved in some receiver class: present only
    /// once stored, so the load is followed by the hole compare, whose miss
    /// is the site's complete generic operation.
    pub maybe_absent: bool,
    /// The read is a value of representation `Arr` (DESIGN-p4l1 §3.2): the
    /// join, over the class set, of what the field holds
    /// (`FactsField::rep`). `None` when some class's field may hold
    /// anything but plain program Arrays, and for a guarded or possibly
    /// absent read, whose value may come from elsewhere.
    pub array: Option<crate::codegen::FactsArrRep>,
    /// The field's Number representation over the class set (I32 at rest,
    /// PR A: wpfacts `FieldFacts::rep` F64 or I32, joined): `None` when
    /// some class's field may hold anything but Numbers, and for a guarded
    /// or possibly absent read. PR A consumes it only in check mode.
    pub num_rep: Option<crate::codegen::FactsRep>,
    /// −0 tolerance (PR D): some class's field may hold a `-0` no use
    /// observes (`ElisionFacts::zero_tolerant_fields`); the field-rep
    /// witness admits it.
    pub num_zero_ok: bool,
    /// Every class's field holds, as Strings, only program string
    /// literals' values (`ElisionFacts::literal_string_fields`): what an
    /// unguarded store writes there is one (`expr::literal_strings`).
    pub literal_strings: bool,
}

impl FieldProof {
    /// Every class holds the field at one slot (§3.7 case 1).
    pub(crate) fn agrees(&self) -> bool {
        self.slots.iter().all(|s| *s == self.slot)
    }

    /// The site tests the proof at run time and has a generic miss arm.
    pub(crate) fn guarded(&self) -> bool {
        self.checked || self.interp_guard || self.maybe_absent
    }

    pub(crate) fn may_hold_string(&self) -> bool {
        self.kind_mask & K_STR != 0
    }

    pub(crate) fn may_hold_pointer(&self) -> bool {
        self.kind_mask & K_POINTER != 0
    }

    pub(crate) fn fact_ids(&self, property: &str) -> String {
        self.class_names
            .iter()
            .map(|c| format!("F3:{c} prefix_stable,F1':{c}.{property} definite,F2:{c}.{property}"))
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// `PERRY_FACTS_ELIDE_TRACE=1`: one stderr line per elided site at compile
/// time (diagnostics).
fn trace_enabled() -> bool {
    use std::sync::OnceLock;
    static T: OnceLock<bool> = OnceLock::new();
    *T.get_or_init(|| std::env::var("PERRY_FACTS_ELIDE_TRACE").as_deref() == Ok("1"))
}

/// The receiver facts of `object`, when it is a binding the facts answer for.
fn receiver(ctx: &FnCtx<'_>, object: &Expr) -> Option<(crate::codegen::FactsRecv, String)> {
    if ctx.wpfacts.is_empty() || ctx.wpfacts_off {
        return None;
    }
    match object {
        Expr::LocalGet(id) => {
            // A scalar-replaced or POD local is not an object at all.
            if ctx.scalar_replaced.contains_key(id) || ctx.pod_records.contains_key(id) {
                return None;
            }
            ctx.wpfacts.locals.get(id).map(|r| {
                crate::facts_consumed::used();
                crate::verify_facts::consumed(
                    crate::verify_facts::Fact::Receiver,
                    || serde_json::json!({"classes":r.class_ids,"nullable":r.nullable,"guarded":r.checked || ctx.wpfacts.interp_guard}),
                );
                (r.clone(), format!("local {id}"))
            })
        }
        Expr::This => {
            if ctx.is_static_class_this(object) {
                return None;
            }
            let top = ctx.this_stack.last()?;
            let (_, fid) = ctx.wpfacts_this.iter().rev().find(|(s, _)| s == top)?;
            ctx.wpfacts.this_of.get(fid).map(|r| {
                crate::facts_consumed::used();
                crate::verify_facts::consumed(
                    crate::verify_facts::Fact::Receiver,
                    || serde_json::json!({"classes":r.class_ids,"nullable":r.nullable,"guarded":r.checked || ctx.wpfacts.interp_guard}),
                );
                (r.clone(), format!("this of fn {fid}"))
            })
        }
        // Holder facts: a read of a field of an unchecked proven receiver
        // holds, in every class of that receiver, only instances of the
        // field's classes (or null/undefined): their union is this value's
        // receiver fact. No guard: an unchecked receiver needs none, and a
        // field with ⊤ or interpreted writers has no classes.
        Expr::PropertyGet {
            object: inner,
            property,
            ..
        } => {
            if property.starts_with('#') || ctx.wpfacts.interp_guard {
                return None;
            }
            let (r, binding) = receiver(ctx, inner)?;
            if r.checked || r.class_ids.is_empty() {
                return None;
            }
            let mut class_ids: Vec<u32> = Vec::new();
            let mut nullable = false;
            for &cid in &r.class_ids {
                let f = ctx.wpfacts.field(cid, property)?;
                crate::verify_facts::consumed(
                    crate::verify_facts::Fact::HolderClasses,
                    || serde_json::json!({"holder":cid,"key":property,"classes":f.classes,"nullable":f.nullable}),
                );
                if f.reserved || f.classes.is_empty() {
                    return None;
                }
                class_ids.extend_from_slice(&f.classes);
                nullable |= f.nullable;
            }
            class_ids.sort_unstable();
            class_ids.dedup();
            Some((
                crate::codegen::FactsRecv {
                    class_ids,
                    nullable,
                    checked: false,
                },
                format!("field {property} of {binding}"),
            ))
        }
        _ => None,
    }
}

/// The join of the field `property`'s representation over the classes
/// `class_ids`, when it is `Arr` in each (`FieldProof::array`).
fn class_set_array(
    ctx: &FnCtx<'_>,
    class_ids: &[u32],
    property: &str,
) -> Option<crate::codegen::FactsArrRep> {
    use crate::codegen::{FactsArrRep, FactsRep};
    let mut out: Option<FactsArrRep> = None;
    for &cid in class_ids {
        let f = ctx.wpfacts.field(cid, property)?;
        let FactsRep::Arr(r) = f.rep else {
            return None;
        };
        if f.declared || f.reserved {
            return None;
        }
        out = Some(out.map_or(r, |o| FactsArrRep::join(o, r)));
    }
    out
}

/// The join of the field `property`'s Number representation (F64 or I32)
/// over the classes `class_ids`, when it is one in each (I32 at rest).
fn class_set_number(
    ctx: &FnCtx<'_>,
    class_ids: &[u32],
    property: &str,
) -> Option<crate::codegen::FactsRep> {
    use crate::codegen::FactsRep;
    let mut out: Option<FactsRep> = None;
    for &cid in class_ids {
        let f = ctx.wpfacts.field(cid, property)?;
        if f.declared || f.reserved || !matches!(f.rep, FactsRep::F64 | FactsRep::I32) {
            return None;
        }
        out = Some(match (out, f.rep) {
            (Some(FactsRep::I32), FactsRep::I32) | (None, FactsRep::I32) => FactsRep::I32,
            _ => FactsRep::F64,
        });
    }
    out
}

/// The representation of the value `object.property` reads, when the facts
/// give it `Arr` (DESIGN-p4l1 §3.2, §4.2): `object` is an unchecked proven
/// receiver, no interpreted code can run, and in every class it may be the
/// field holds only plain program Arrays for the object's whole life. It
/// is `FieldProof::array` without the proof's slot: the field's value is
/// the same whichever way the read is lowered.
pub(crate) fn field_array(
    ctx: &FnCtx<'_>,
    object: &Expr,
    property: &str,
) -> Option<crate::codegen::FactsArrRep> {
    if property.starts_with('#') || ctx.wpfacts.interp_guard {
        return None;
    }
    let (r, _) = receiver(ctx, object)?;
    if r.checked {
        return None;
    }
    class_set_array(ctx, &r.class_ids, property)
}

/// FVA: the class ids the facts prove a call's receiver to be one of (an
/// unchecked binding, no interpreted code), and whether it may also be
/// null or undefined (then the site keeps that receiver test, as a read
/// does). A virtual call over them is the T0set read shape.
pub(crate) fn proven_receiver_classes(ctx: &FnCtx<'_>, object: &Expr) -> Option<(Vec<u32>, bool)> {
    let (r, _) = receiver(ctx, object)?;
    (!r.checked && !ctx.wpfacts.interp_guard && !r.class_ids.is_empty())
        .then(|| (r.class_ids, r.nullable && !tested_in_block(ctx, object)))
}

pub(crate) fn receiver_checked(ctx: &FnCtx<'_>, object: &Expr) {
    if crate::verify_facts::enabled() {
        if let Some((r, _)) = receiver(ctx, object) {
            crate::verify_facts::covered(
                crate::verify_facts::Fact::Receiver,
                || serde_json::json!({"classes":r.class_ids,"nullable":r.nullable,"guarded":r.checked || ctx.wpfacts.interp_guard}),
            );
        }
    }
}

/// FVA 3.4: the facts prove class `cid`'s own static `key` holds its
/// definition for the program's life (no write, no escape of the class
/// object, no interpreted code).
pub(crate) fn proves_static(ctx: &FnCtx<'_>, cid: u32, key: &str) -> bool {
    let proved = !ctx.wpfacts_off && facts_static(&ctx.wpfacts, cid, key);
    if proved {
        crate::verify_facts::consumed(
            crate::verify_facts::Fact::Static,
            || serde_json::json!({"class":cid,"key":key}),
        );
        crate::facts_consumed::used();
    }
    proved
}

/// The whole-program half of [`proves_static`], from the facts alone (the
/// LIFE census resolves a static call by it): no interpreted code, and the
/// facts list `key` among class `cid`'s own statics that hold their
/// definition.
pub(crate) fn facts_static(facts: &crate::codegen::ElisionFacts, cid: u32, key: &str) -> bool {
    !facts.interp_guard
        && facts
            .class_statics
            .get(&cid)
            .is_some_and(|ks| ks.iter().any(|k| k == key))
}

/// The class id of a receiver the facts prove one of their classes: one
/// load of its header word (the read shape's `select_by_class` load). A
/// `nullable` receiver tests its tag first and gives 0 when it is nullish
/// (no class id is 0).
pub(crate) fn emit_class_id(ctx: &mut FnCtx<'_>, recv_box: &str, nullable: bool) -> String {
    let load = |ctx: &mut FnCtx<'_>| {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(recv_box);
        let biased = blk.sub(
            I64,
            &bits,
            &(super::receiver_range::RECEIVER_BIAS as i64).to_string(),
        );
        let hdr = super::receiver_range::emit_field_ptr(blk, &biased, 0);
        blk.load(I32, &hdr)
    };
    if !nullable {
        return load(ctx);
    }
    let is_obj = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(recv_box);
        let top16 = blk.lshr(I64, &bits, "48");
        blk.icmp_eq(I64, &top16, crate::nanbox::POINTER_TAG_TOP16_I64)
    };
    let from = ctx.block().label.clone();
    let load_idx = ctx.new_block("proven_recv.class_id");
    let join_idx = ctx.new_block("proven_recv.join");
    let load_label = ctx.block_label(load_idx);
    let join_label = ctx.block_label(join_idx);
    ctx.block().cond_br(&is_obj, &load_label, &join_label);
    ctx.current_block = load_idx;
    let cid = load(ctx);
    let loaded = ctx.block().label.clone();
    ctx.block().br(&join_label);
    ctx.current_block = join_idx;
    ctx.block().phi(
        I32,
        &[(cid.as_str(), loaded.as_str()), ("0", from.as_str())],
    )
}

/// A local that is never reassigned in this function holds one value for
/// the whole body, so once an earlier access in the CURRENT basic block
/// passed its receiver test (whose other arm does not return), every later
/// access in that block sees an object. `this` never changes either.
fn stable_binding(ctx: &FnCtx<'_>, object: &Expr) -> Option<i64> {
    match object {
        Expr::LocalGet(id)
            if !ctx.reassigned_locals.contains(id)
                && !ctx.boxed_vars.contains(id)
                && !ctx.closure_captures.contains_key(id) =>
        {
            Some(*id as i64)
        }
        _ => None,
    }
}

/// After a receiver test passed in the current block: later accesses of the
/// same stable binding in this block see an object.
pub(crate) fn note_tested(ctx: &mut FnCtx<'_>, object: &Expr) {
    if let Some(b) = stable_binding(ctx, object) {
        let blk = ctx.current_block;
        ctx.wpfacts_tested.push((b, blk));
    }
}

fn tested_in_block(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    stable_binding(ctx, object).is_some_and(|b| {
        ctx.wpfacts_tested
            .iter()
            .any(|&(t, blk)| t == b && blk == ctx.current_block)
    })
}

/// A number-context read (`lower_raw_f64_class_field_get_for_number_context`,
/// `expr_produces_canonical_raw_f64`, `is_numeric_expr`): the facts prove the
/// field only ever holds Numbers, so [`try_lower_get`] yields the slot's
/// word, which is the Number's canonical double.
pub(crate) fn proves_number(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    let Expr::PropertyGet {
        object, property, ..
    } = expr
    else {
        return false;
    };
    prove(ctx, object, property, Access::Get).is_some_and(|p| p.number)
}

/// The receiver is not a heap object: a scalar-replaced local, or `this` of
/// a constructor inlined into one. Its reads take the scalar slot route in
/// `property_get`, not [`try_lower_get`], so the proof does not select them.
fn scalar_receiver(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    match object {
        Expr::LocalGet(id) => ctx.scalar_replaced.contains_key(id),
        Expr::This => !ctx.scalar_ctor_target.is_empty(),
        _ => false,
    }
}

/// The local class of class id `cid` (its own name, not an alias).
pub(crate) fn class_name_of(ctx: &FnCtx<'_>, cid: u32) -> Option<String> {
    ctx.classes
        .iter()
        .find(|(name, c)| c.id == cid && c.name == **name)
        .map(|(name, _)| name.clone())
}

/// The whole-program half of a field read's proof, from the facts alone:
/// the receiver binding's class set is known and small, and in every class
/// the key is a birth data field. `prove` gates on it before it consults
/// codegen's own layout; the parameter fate (`life_fate`) asks it alone, so
/// a read the fate calls code-free is a read `prove` admits on the same
/// predicate. `Ok(true)`: the read is a bare load with no run-time test and
/// no miss arm (unchecked binding, no interpreted code, every class's field
/// definite); `Ok(false)`: proven only behind a guard or a hole compare,
/// whose miss arm is the generic operation (it may run code). `Err`: not
/// proven, with `prove`'s refusal reason.
pub(crate) fn facts_read(
    facts: &crate::codegen::ElisionFacts,
    recv: &crate::codegen::FactsRecv,
    property: &str,
) -> Result<bool, &'static str> {
    if property.starts_with('#') {
        return Err("private key requires private-element semantics");
    }
    if recv.class_ids.is_empty() || recv.class_ids.len() > 4 {
        return Err("receiver class set is empty or exceeds four classes");
    }
    let mut bare = !recv.checked && !facts.interp_guard;
    for &cid in &recv.class_ids {
        let f = facts
            .field(cid, property)
            .ok_or("key is not a definite data birth field of every receiver class")?;
        bare &= !f.declared && !f.reserved;
    }
    Ok(bare)
}

/// The zero-guard proof for `object.property`, if the facts give one.
pub(crate) fn prove(
    ctx: &FnCtx<'_>,
    object: &Expr,
    property: &str,
    access: Access,
) -> Option<FieldProof> {
    if property.starts_with('#') {
        crate::fallback_diag::refuse("private key requires private-element semantics");
        return None;
    }
    // Inside an element-shape fast clone a tracked read belongs to its loop
    // fact (`element_shape_loop_fact_for_property_get`). The clone's element
    // binding (`const o = a[i]`) is virtual and binds no slot, so a proof here
    // would lower `o` itself and read undefined. Not a fallback: the loop
    // fact lowers the read.
    if crate::expr::element_shape_loop_fact_for_property_get(ctx, object, property).is_some() {
        return None;
    }
    let (mut recv, binding) = receiver(ctx, object).or_else(|| {
        crate::fallback_diag::refuse("receiver has no prefix-stable binding proof");
        None
    })?;
    if let Err(why) = facts_read(&ctx.wpfacts, &recv, property) {
        crate::fallback_diag::refuse(why);
        return None;
    }
    let checked = recv.checked;
    let interp_guard = ctx.wpfacts.interp_guard;
    if (checked || interp_guard) && access != Access::Get {
        crate::fallback_diag::refuse(
            "guarded field proof is read-only; store/update needs checked semantics",
        );
        return None;
    }
    if recv.nullable && tested_in_block(ctx, object) {
        recv.nullable = false;
    }
    // CLS D3: a plain store keeps the receiver test (nullish throws in the
    // complete `[[Set]]`, which is the test's miss arm); an update does not.
    if recv.nullable && access == Access::Update {
        crate::fallback_diag::refuse("nullable update receiver requires generic nullish semantics");
        return None;
    }
    let mut slot: Option<u32> = None;
    let mut maybe_absent = false;
    let mut kind_mask = 0u16;
    let mut ta_stable = true;
    let mut ta_kind: Option<Option<u8>> = None;
    let mut all_f64 = true;
    let mut any_f64 = false;
    let mut names = Vec::new();
    let mut slots = Vec::new();
    for &cid in &recv.class_ids {
        let f = ctx.wpfacts.field(cid, property).or_else(|| {
            crate::fallback_diag::refuse(
                "key is not a definite data birth field of every receiver class",
            );
            None
        })?;
        if f.declared {
            // A declared key lives at its family slot: the object is born on
            // the family layout and keeps it for life, so the program-wide
            // layout is the authority (the class may be another module's).
            // Stores add the key on the checked route.
            let fam_slot = ctx.program_layouts.families.get(&cid).and_then(|fam| {
                fam.keys
                    .iter()
                    .find(|(k, _)| *k == property)
                    .map(|(_, s)| *s)
            });
            let Some(idx) = fam_slot.filter(|_| access == Access::Get) else {
                crate::fallback_diag::refuse(
                    "declared key: stores add it on the checked route; reads need the family",
                );
                return None;
            };
            maybe_absent = true;
            slot.get_or_insert(idx);
            slots.push(idx);
            kind_mask |= f.kind_mask;
            ta_stable = false;
            ta_kind = Some(None);
            all_f64 = false;
            names.push(format!("class#{cid}"));
            continue;
        }
        let name = class_name_of(ctx, cid).or_else(|| {
            crate::fallback_diag::refuse("receiver class is absent from codegen class metadata");
            None
        })?;
        // Codegen's own layout must agree with the facts' birth slot.
        let idx =
            crate::type_analysis::class_field_global_index(ctx, &name, property).or_else(|| {
                crate::fallback_diag::refuse("class field has no codegen slot");
                None
            })?;
        if idx != f.slot {
            crate::fallback_diag::refuse("facts slot disagrees with codegen layout");
            return None;
        }
        if super::property_get::class_has_evaluated_accessor(ctx, &name)
            || super::class_field_inline_guard::class_instances_carry_private_elements(ctx, &name)
            || super::class_field_inline_guard::class_instances_grow_past_layout(ctx, &name)
        {
            crate::fallback_diag::refuse(
                "computed/private/growing class layout refuses fixed-offset field access",
            );
            return None;
        }
        if f.reserved {
            // The slot holds the hole until the constructor stores the key
            // (object model §3.2: presence is the slot word). A read pairs
            // the load with the hole compare; a store fills the hole, which
            // adds the key, on the checked route. A raw-f64 lane has no
            // hole word.
            if access != Access::Get
                || super::class_field_inline_guard::class_birth_slot_is_f64(ctx, &name, idx)
            {
                crate::fallback_diag::refuse(
                    "reserved key: stores add it on the checked route; reads need a boxed slot",
                );
                return None;
            }
            maybe_absent = true;
            slot.get_or_insert(idx);
            slots.push(idx);
            kind_mask |= f.kind_mask;
            ta_stable = false;
            ta_kind = Some(None);
            all_f64 = false;
            names.push(name);
            continue;
        }
        if access != Access::Get {
            let keys_global = ctx.class_keys_globals.get(&name)?;
            if crate::codegen::slot_may_be_constfn(keys_global, idx) {
                crate::fallback_diag::refuse("constant-function lane requires checked store");
                return None;
            }
        }
        slot.get_or_insert(idx);
        slots.push(idx);
        kind_mask |= f.kind_mask;
        ta_stable &= f.ta_stable;
        ta_kind = match ta_kind {
            None => Some(f.ta_kind),
            Some(k) if k == f.ta_kind => Some(k),
            Some(_) => Some(None),
        };
        let lane = super::class_field_inline_guard::class_birth_slot_is_f64(ctx, &name, idx);
        all_f64 &= lane;
        any_f64 |= lane;
        names.push(name);
    }
    if maybe_absent {
        crate::verify_facts::consumed(
            crate::verify_facts::Fact::Presence,
            || serde_json::json!({"classes":recv.class_ids,"key":property}),
        );
    }
    if ta_stable {
        crate::verify_facts::consumed(
            crate::verify_facts::Fact::TypedOwner,
            || serde_json::json!({"classes":recv.class_ids,"key":property,"kind":ta_kind.flatten()}),
        );
    }
    if interp_guard {
        crate::verify_facts::consumed(crate::verify_facts::Fact::InterpreterGuard, || {
            serde_json::json!({})
        });
    }
    let guarded = checked || interp_guard;
    let proof = FieldProof {
        class_ids: recv.class_ids.clone(),
        class_names: names,
        slot: slot?,
        slots,
        nullable: recv.nullable,
        kind_mask,
        // A guarded read's value may come from its generic arm.
        ta_stable: ta_stable && !guarded,
        ta_kind: if guarded { None } else { ta_kind.flatten() },
        raw_f64: all_f64 && kind_mask == K_NUM && !guarded,
        number: kind_mask == K_NUM
            && !guarded
            && !maybe_absent
            && (all_f64 || !scalar_receiver(ctx, object)),
        any_f64_lane: any_f64,
        binding,
        checked,
        interp_guard,
        maybe_absent,
        array: if guarded || maybe_absent {
            None
        } else {
            class_set_array(ctx, &recv.class_ids, property)
        },
        num_rep: if guarded || maybe_absent {
            None
        } else {
            class_set_number(ctx, &recv.class_ids, property)
        },
        num_zero_ok: recv.class_ids.iter().any(|&c| {
            ctx.wpfacts
                .zero_tolerant_fields
                .contains(&(c, property.to_string()))
        }),
        literal_strings: !guarded
            && !maybe_absent
            && recv.class_ids.iter().all(|&c| {
                ctx.wpfacts
                    .literal_string_fields
                    .contains(&(c, property.to_string()))
            }),
    };
    crate::verify_facts::field_consumed(&proof, property);
    Some(proof)
}

/// A stable id for one elided site (printed by the witness on a violation,
/// and the key of its `.elisions` line).
fn site_id(ctx: &mut FnCtx<'_>, property: &str) -> u64 {
    let n = ctx.wpfacts_sites;
    ctx.wpfacts_sites += 1;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in ctx
        .source_function_slug
        .bytes()
        .chain(ctx.func.name.bytes())
        .chain(property.bytes())
        .chain(n.to_le_bytes())
    {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Record the elision (design 6.1): `<PERRY_FACTS_DUMP>.elisions`.
fn record(ctx: &FnCtx<'_>, site: u64, what: &str, property: &str, p: &FieldProof) {
    let line = format!(
        "E {site:#018x} {what} fn={} .{property} recv={} classes={:?} slot={} nullable={} kind_mask={:#x} raw_f64={} number={} proof={} facts={}\n",
        ctx.func.name,
        p.binding,
        p.class_names,
        if p.agrees() {
            p.slot.to_string()
        } else {
            format!("{:?}", p.slots)
        },
        p.nullable as u8,
        p.kind_mask,
        p.raw_f64 as u8,
        p.number as u8,
        match (p.checked, p.interp_guard) {
            (true, _) => "checked",
            (false, true) => "guarded",
            (false, false) => "unchecked",
        },
        p.fact_ids(property)
    );
    if trace_enabled() {
        eprint!("perry: wpfacts elide: {line}");
    }
    if let Some(dump) = std::env::var_os("PERRY_FACTS_DUMP") {
        let mut path = dump;
        path.push(".elisions");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            use std::io::Write;
            let _ = f.write_all(line.as_bytes());
        }
    }
}

fn pack_cids(ids: &[u32]) -> (String, String) {
    let at = |i: usize| ids.get(i).copied().unwrap_or(u32::MAX) as u64;
    let a = (at(0) | (at(1) << 32)) as i64;
    let b = (at(2) | (at(3) << 32)) as i64;
    (a.to_string(), b.to_string())
}

/// The contract witness of one elided access (contract builds only).
pub(crate) fn emit_witness(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    value: &str,
    property: &str,
    p: &FieldProof,
    flags: u32,
    site: u64,
) {
    if !crate::verify_facts::witnesses() {
        return;
    }
    crate::verify_facts::field_checked(p, property);
    let what = format!(
        "{site:#018x} {} .{property} classes={:?} class_ids={:?} slots={:?}",
        if flags & WPF_STORE != 0 {
            "store"
        } else {
            "read"
        },
        p.class_names,
        p.class_ids,
        p.slots
    );
    if crate::verify_facts::enabled() {
        let site_arg =
            crate::verify_facts::raw_site(ctx, "field", Some(property), &p.class_ids, &what);
        let (key, key_len) =
            crate::verify_facts::raw_bytes(ctx.strings.module_prefix(), property.as_bytes());
        let (c01, c23) = pack_cids(&p.class_ids);
        let (triple, form) = (ctx.target_triple, p.form());
        let off_of = |s: u32| crate::target_layout::layout_key_off(triple, form, s).to_string();
        let (slot, off) = if p.agrees() {
            (p.slot.to_string(), off_of(p.slot))
        } else {
            let bits = ctx.block().bitcast_double_to_i64(recv_box);
            let biased = ctx.block().sub(
                I64,
                &bits,
                &(super::receiver_range::RECEIVER_BIAS as i64).to_string(),
            );
            (
                select_by_class(ctx, &biased, p, |s| s.to_string()),
                select_by_class(ctx, &biased, p, &off_of),
            )
        };
        let flags = flags
            | if p.nullable { WPF_NULLABLE } else { 0 }
            | if p.raw_f64 && flags & WPF_STORE == 0 {
                WPF_RAW_F64
            } else {
                0
            };
        ctx.block().call(
            I32,
            "perry_verify_facts_field",
            &[
                (DOUBLE, recv_box),
                (DOUBLE, value),
                (I64, &key),
                (I64, &key_len),
                (I64, &c01),
                (I64, &c23),
                (I32, &slot),
                (I32, &off),
                (I32, &(p.kind_mask as i32).to_string()),
                (I32, &flags.to_string()),
                (I64, &site_arg.pointer),
                (I64, site_arg.len.as_deref().unwrap()),
            ],
        );
        return;
    }
    let site_arg =
        crate::verify_facts::site_arg(ctx, &what, crate::verify_facts::Fact::Slot, &p.class_ids);
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let (c01, c23) = pack_cids(&p.class_ids);
    // The slot names the key (F3); its byte offset is the address the
    // access used, which the witness reads.
    let (triple, form) = (ctx.target_triple, p.form());
    let off_of = |s: u32| crate::target_layout::layout_key_off(triple, form, s).to_string();
    let (slot, off) = if p.agrees() {
        (p.slot.to_string(), off_of(p.slot))
    } else {
        let bits = ctx.block().bitcast_double_to_i64(recv_box);
        let biased = ctx.block().sub(
            I64,
            &bits,
            &(super::receiver_range::RECEIVER_BIAS as i64).to_string(),
        );
        let slot = select_by_class(ctx, &biased, p, |s| s.to_string());
        (slot, select_by_class(ctx, &biased, p, &off_of))
    };
    let blk = ctx.block();
    let key_box = blk.load(DOUBLE, &key_handle_global);
    let key_bits = blk.bitcast_double_to_i64(&key_box);
    let key_handle = blk.and(I64, &key_bits, POINTER_MASK_I64);
    let flags = flags
        | if p.nullable { WPF_NULLABLE } else { 0 }
        | if p.raw_f64 && flags & WPF_STORE == 0 {
            WPF_RAW_F64
        } else {
            0
        };
    // Not a GC leaf (the key list read can reach a collector through the
    // runtime's array-length helper): an ordinary, statepointed call.
    let _ = blk.call(
        I32,
        "perry_wpfacts_check_field",
        &[
            (DOUBLE, recv_box),
            (DOUBLE, value),
            (I64, &key_handle),
            (I64, &c01),
            (I64, &c23),
            (I32, &slot),
            (I32, &off),
            (I32, &(p.kind_mask as i32).to_string()),
            (I32, &flags.to_string()),
            (I64, &site_arg),
        ],
    );
}

/// The slot pointer of a receiver known to be an object, addressed from the
/// receiver's biased value (`receiver_range`): the handle floor and the slot
/// offset fold into the load's displacement. (Masking the NaN-box with
/// `POINTER_MASK` instead lowers to `bzhi` with an 8-bit count register on
/// BMI2 targets, whose partial-register write makes the load wait on an
/// unrelated earlier value: binary-trees arena +8% cycles.)
pub(crate) fn emit_slot_ptr(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    form: crate::target_layout::HeaderForm,
    slot: u32,
) -> String {
    let header = crate::target_layout::object_slot_base_bytes(ctx.target_triple, form) as i64;
    let blk = ctx.block();
    let obj_bits = blk.bitcast_double_to_i64(recv_box);
    let biased = blk.sub(
        I64,
        &obj_bits,
        &(super::receiver_range::RECEIVER_BIAS as i64).to_string(),
    );
    super::receiver_range::emit_field_ptr(blk, &biased, header + i64::from(slot) * 8)
}

/// `i32` `arm(slot)` of the receiver's class (§3.7 case 2): the header's
/// class id against the proof's class ids, constants of the compile. The
/// last class is the default arm: the proof says the receiver is one of
/// them.
fn select_by_class(
    ctx: &mut FnCtx<'_>,
    biased: &str,
    p: &FieldProof,
    arm: impl Fn(u32) -> String,
) -> String {
    let blk = ctx.block();
    let hdr = super::receiver_range::emit_field_ptr(blk, biased, 0);
    let cid = blk.load(I32, &hdr);
    let n = p.class_ids.len();
    let mut v = arm(p.slots[n - 1]);
    for i in (0..n - 1).rev() {
        if p.slots[i] == p.slots[n - 1] {
            continue;
        }
        let eq = blk.icmp_eq(I32, &cid, &p.class_ids[i].to_string());
        v = blk.select(crate::types::I1, &eq, I32, &arm(p.slots[i]), &v);
    }
    v
}

/// The slot pointer of an elided access: [`emit_slot_ptr`] when every class
/// agrees on the slot (case 1), else the slot selected by the receiver's
/// class id (case 2).
pub(crate) fn emit_proof_slot_ptr(ctx: &mut FnCtx<'_>, recv_box: &str, p: &FieldProof) -> String {
    if p.agrees() {
        return emit_slot_ptr(ctx, recv_box, p.form(), p.slot);
    }
    let header = crate::target_layout::object_slot_base_bytes(ctx.target_triple, p.form()) as i64;
    let biased = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(recv_box);
        blk.sub(
            I64,
            &bits,
            &(super::receiver_range::RECEIVER_BIAS as i64).to_string(),
        )
    };
    let off = select_by_class(ctx, &biased, p, |s| (header + i64::from(s) * 8).to_string());
    let blk = ctx.block();
    let off64 = blk.sext(I32, &off, I64);
    let base = super::receiver_range::emit_field_ptr(blk, &biased, 0);
    blk.gep(I8, &base, &[(I64, &off64)])
}

/// `object.property` read through a zero-guard proof.
pub(crate) fn try_lower_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    byte_offset: u32,
) -> Result<Option<String>> {
    let Some(p) = prove(ctx, object, property, Access::Get) else {
        return Ok(None);
    };
    if p.guarded() {
        return lower_guarded_get(ctx, object, property, byte_offset, p).map(Some);
    }
    let site = site_id(ctx, property);
    crate::expr::note_site(if p.agrees() {
        "case1-wpfacts"
    } else {
        "case2-wpfacts"
    });
    let recv_box = lower_expr(ctx, object)?;
    if p.nullable {
        // The binding holds an object or null/undefined (F4). The one test
        // that stays sends null/undefined to the complete site operation,
        // which throws the TypeError and so never returns here: the arm ends
        // in `unreachable`, and every later access of the binding on this
        // path sees an object without testing again.
        crate::expr::calls::emit_call_location_at(ctx, byte_offset);
        let hit_idx = ctx.new_block("wpf.get.obj");
        let miss_idx = ctx.new_block("wpf.get.nullish");
        let hit_label = ctx.block_label(hit_idx);
        let miss_label = ctx.block_label(miss_idx);
        let obj_bits = ctx.block().bitcast_double_to_i64(&recv_box);
        let recv = super::receiver_range::emit_fused_receiver_test(ctx.block(), &obj_bits);
        ctx.block()
            .cond_br(&recv.is_object_pointer, &hit_label, &miss_label);
        ctx.current_block = miss_idx;
        emit_witness(ctx, &recv_box, &recv_box, property, &p, 0, site);
        let feedback_site_id = super::emit_typed_feedback_register_site(
            ctx,
            super::TypedFeedbackKind::PropertyGet,
            property,
            super::TypedFeedbackContract::object_get_by_name(),
        );
        let key_idx = ctx.strings.intern(property);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let blk = ctx.block();
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_bits = blk.bitcast_double_to_i64(&key_box);
        let key_handle = blk.and(I64, &key_bits, POINTER_MASK_I64);
        // A non-object receiver: the complete read throws for nullish.
        let _ = blk.call(
            DOUBLE,
            "js_object_get_field_value",
            &[
                (I64, &obj_bits),
                (I64, &key_handle),
                (I64, &feedback_site_id),
            ],
        );
        blk.unreachable();
        ctx.current_block = hit_idx;
        if let Some(b) = stable_binding(ctx, object) {
            ctx.wpfacts_tested.push((b, hit_idx));
        }
    }
    let value = {
        let field_ptr = emit_proof_slot_ptr(ctx, &recv_box, &p);
        let val = ctx.block().load(DOUBLE, &field_ptr);
        emit_witness(ctx, &recv_box, &val, property, &p, 0, site);
        // The receiver of a fill push reads the Array while it is being born
        // (wpfacts `push_fills`): its claim holds once the fill is over.
        let filling = matches!(object, Expr::This)
            && byte_offset != 0
            && ctx.wpfacts.fill_push_recvs.contains(&byte_offset);
        if let Some(r) = p
            .array
            .filter(|_| crate::verify_facts::enabled() && !filling)
        {
            // Check mode: the field's `Arr` claim, on the word just read.
            super::index_get::array_rep::witness_value(
                ctx,
                &val,
                r,
                &format!("field {property} of {}", p.binding),
            );
        }
        if let Some(r) = p.num_rep {
            // Check mode (I32 at rest): the field's Number rep.
            crate::codegen::signature_entry::witness_field_rep(
                ctx,
                r,
                &val,
                &format!("{property} of {}", p.binding),
                p.num_zero_ok,
            );
        }
        // A Number has one encoding in every JS value (the raw double), so
        // a read the facts prove is a Number is the loaded word: no tag
        // test, no select, no branch.
        val
    };
    let numeric = p.number;
    let (semantic, rep) = if numeric {
        (SemanticKind::JsNumber, NativeRep::F64)
    } else {
        (SemanticKind::JsValue, NativeRep::JsValue)
    };
    ctx.record_lowered_value_with_access_mode_and_facts(
        "ClassFieldGet",
        None,
        "class_field_get.wpfacts_t0",
        &LoweredValue {
            semantic,
            rep,
            llvm_ty: DOUBLE,
            value: value.clone(),
        },
        Some(BoundsState::Guarded {
            guard_id: "wpfacts_t0".to_string(),
        }),
        None,
        Some(BufferAccessMode::CheckedNative),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
        false,
        false,
        vec![
            format!("field={property}"),
            format!("field_index={}", p.slot),
            format!("receiver_proof=wpfacts:{}", p.binding),
        ],
    );
    record(ctx, site, "get", property, &p);
    Ok(Some(value))
}

/// A guarded read (decision 150): the hit arm is the proof's fixed-offset
/// load; every test the proof needs at run time sends a miss to the site's
/// complete generic operation, lowered without the facts, and both arms
/// rejoin.
///
/// The tests, all in the entry block of the site: the runtime's
/// `PERRY_INTERP_PROTO_WRITTEN` word is 0 (`interp_guard`: no interpreted
/// code has written a prototype, so every birth field was defined and no
/// object reached interpreted code by a prototype member); the receiver is
/// an object (`checked` or `nullable`); its class id is one of the proof's
/// (`checked`; the ids are constants of the compile, nothing is learned).
///
/// A checked site whose key the layout record serves tries the record arm
/// (`layout_read::begin`) first: it hits every receiver whose layout holds
/// the key, class instances, literals and parsed objects alike. The class-id
/// arm runs only on its miss (a class whose key is not at its colour slot),
/// and the ordinary site, on the already evaluated receiver, on the class-id
/// arm's miss.
fn lower_guarded_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    byte_offset: u32,
    p: FieldProof,
) -> Result<String> {
    let site = site_id(ctx, property);
    crate::expr::note_site(if p.checked {
        "case2-wpfacts-checked"
    } else {
        "case1-wpfacts-guarded"
    });
    let recv_box = lower_expr(ctx, object)?;
    let record_arm = if p.checked {
        let bits = ctx.block().bitcast_double_to_i64(&recv_box);
        crate::expr::layout_read::begin(ctx, &bits, property, None)
    } else {
        None
    };
    let hit_idx = ctx.new_block("wpf.gget.hit");
    let miss_idx = ctx.new_block("wpf.gget.miss");
    let done_idx = ctx.new_block("wpf.gget.done");
    let hit_label = ctx.block_label(hit_idx);
    let miss_label = ctx.block_label(miss_idx);
    let done_label = ctx.block_label(done_idx);
    let mut ok: Option<String> = None;
    if p.interp_guard {
        let blk = ctx.block();
        let w = blk.load(I32, "@PERRY_INTERP_PROTO_WRITTEN");
        ok = Some(blk.icmp_eq(I32, &w, "0"));
    }
    if p.checked || p.nullable {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(&recv_box);
        let recv = super::receiver_range::emit_fused_receiver_test(blk, &bits);
        let both = match ok.take() {
            Some(prev) => blk.and(crate::types::I1, &prev, &recv.is_object_pointer),
            None => recv.is_object_pointer.clone(),
        };
        if p.checked {
            // The class id is read only on the object edge.
            let chk_idx = ctx.new_block("wpf.gget.cid");
            let chk_label = ctx.block_label(chk_idx);
            ctx.block().cond_br(&both, &chk_label, &miss_label);
            ctx.current_block = chk_idx;
            // A literal family keeps its late keys and names class 0: its
            // objects carry the family descriptor's layout word, not a class
            // id (every other class is told by its class id alone).
            let literals: Vec<u32> = p
                .class_ids
                .iter()
                .copied()
                .filter(|c| ctx.program_layouts.literal_families.contains(c))
                .collect();
            let literal_families: Vec<String> = literals
                .into_iter()
                .filter_map(|c| crate::lower_call::direct_method_guard::family_layout_id(ctx, c))
                .collect();
            let blk = ctx.block();
            // Word 0 is an object's class id but also an array's or a
            // string's `length`: `[x]` would pass for a class whose id is 1.
            // Only a GC_TYPE_OBJECT carries a class id there.
            let type_ptr = super::receiver_range::emit_field_ptr(
                blk,
                &recv.biased,
                -(perry_abi::GC_HEADER_SIZE as i64),
            );
            let type_byte = blk.load(I8, &type_ptr);
            let is_object = crate::target_layout::emit_gc_type_is_object(blk, I8, &type_byte);
            let hdr = super::receiver_range::emit_field_ptr(blk, &recv.biased, 0);
            let cid = blk.load(I32, &hdr);
            let mut any: Option<String> = None;
            for c in &p.class_ids {
                let eq = blk.icmp_eq(I32, &cid, &c.to_string());
                any = Some(match any {
                    Some(prev) => blk.or(crate::types::I1, &prev, &eq),
                    None => eq,
                });
            }
            if !literal_families.is_empty() {
                let word_ptr = super::receiver_range::emit_field_ptr(blk, &recv.biased, 4);
                let word = blk.load(I32, &word_ptr);
                for id in &literal_families {
                    let eq = blk.icmp_eq(I32, &word, id);
                    // 0: the runtime could not register the layout.
                    let registered = blk.icmp_ne(I32, id, "0");
                    let eq = blk.and(crate::types::I1, &eq, &registered);
                    any = Some(match any {
                        Some(prev) => blk.or(crate::types::I1, &prev, &eq),
                        None => eq,
                    });
                }
            }
            ok = any.map(|any| blk.and(crate::types::I1, &is_object, &any));
        } else {
            ok = Some(both);
        }
    }
    match ok {
        Some(c) => ctx.block().cond_br(&c, &hit_label, &miss_label),
        None => ctx.block().br(&hit_label),
    }
    ctx.current_block = hit_idx;
    let fast = {
        let field_ptr = emit_proof_slot_ptr(ctx, &recv_box, &p);
        let val = ctx.block().load(DOUBLE, &field_ptr);
        emit_witness(ctx, &recv_box, &val, property, &p, 0, site);
        val
    };
    if p.maybe_absent {
        // A declared key holds the hole until stored: an absent key reads
        // through the prototype chain, which is the generic operation.
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(&fast);
        let hole = blk.icmp_eq(
            I64,
            &bits,
            &crate::nanbox::i64_literal(crate::nanbox::TAG_HOLE),
        );
        blk.cond_br(&hole, &miss_label, &done_label);
    } else {
        ctx.block().br(&done_label);
    }
    let fast_end = ctx.block().label.clone();
    ctx.current_block = miss_idx;
    let slow = if record_arm.is_some() {
        // The record arm has missed: the ordinary site, without a second
        // record arm and without lowering the receiver again.
        crate::expr::calls::emit_call_location_at(ctx, byte_offset);
        super::property_get::generic_dispatch::lower_property_site_tail(
            ctx,
            property,
            None,
            recv_box.clone(),
        )?
    } else {
        let generic = Expr::PropertyGet {
            byte_end: 0,
            object: Box::new(object.clone()),
            property: property.to_string(),
            byte_offset,
        };
        let was_off = std::mem::replace(&mut ctx.wpfacts_off, true);
        let slow = lower_expr(ctx, &generic);
        ctx.wpfacts_off = was_off;
        slow?
    };
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&done_label);
    ctx.current_block = done_idx;
    let value = ctx
        .block()
        .phi(DOUBLE, &[(&fast, &fast_end), (&slow, &slow_end)]);
    record(ctx, site, "get", property, &p);
    Ok(match record_arm {
        Some(arm) => crate::expr::layout_read::finish(ctx, arm, &value),
        None => value,
    })
}

/// The proof of an elided store, with its site id (the store emission lives
/// in `property_set.rs`, shared with the `Ptr<Shape>` route).
pub(crate) struct StoreProof {
    pub proof: FieldProof,
    pub site: u64,
}

pub(crate) fn prove_store(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    access: Access,
) -> Option<StoreProof> {
    let proof = prove(ctx, object, property, access)?;
    let site = site_id(ctx, property);
    record(
        ctx,
        site,
        if access == Access::Update {
            "update"
        } else {
            "set"
        },
        property,
        &proof,
    );
    Some(StoreProof { proof, site })
}

/// F6 at an element access `o.f[i]`: the element kind of the typed array
/// `o.f` reads, when `o.f` is an elided read of a field that holds only
/// stable typed-array owners of one kind and nothing else (no
/// null/undefined). The element access may then skip its receiver test and
/// owner-brand test; the bounds test stays (a detached owner has length 0).
pub(crate) fn ta_field_kind(ctx: &FnCtx<'_>, ta_expr: &Expr) -> Option<u8> {
    // `PERRY_FACTS_ELIDE_TA=0`: keep the element tests (A/B control).
    if std::env::var("PERRY_FACTS_ELIDE_TA").as_deref() == Ok("0") {
        return None;
    }
    let Expr::PropertyGet {
        object, property, ..
    } = ta_expr
    else {
        return None;
    };
    let p = prove(ctx, object, property, Access::Get)?;
    (p.ta_stable && !p.nullable && p.kind_mask == K_OBJ)
        .then_some(p.ta_kind)
        .flatten()
}

impl FieldProof {
    /// The header form predicted for the objects this record describes (`HeaderForm`:
    /// every ordinary object has the meta word today). Sites take their slot base
    /// from here, never from the type byte.
    pub(crate) fn form(&self) -> crate::target_layout::HeaderForm {
        crate::target_layout::HeaderForm::Meta
    }
}

#[cfg(test)]
#[path = "wpfacts_access_tests.rs"]
mod tests;
