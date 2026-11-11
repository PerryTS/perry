//! I32 at rest, PR C (representation RFC §10.7; DESIGN-n1-at-rest §4, §6):
//! the I32 field lanes of a class and every site of their keys.
//!
//! wpfacts (`rest_fields`) admits a class's I32-rep fields (`FactsField::rest`)
//! when no generic reader can reach its instances and the class has no
//! heritage. [`class_lanes`] is codegen's one decision of a class's lanes from
//! those facts and the class: every birth path and every site reads it.
//!
//! - **The lane** is an identity fact of the shape: the `11` lane outside the
//!   ConstFn mask (`perry-runtime` `field_rep::i32_lane_slots`), whose slot
//!   holds the I32 word `INT32_TAG | (u32)v`. A class born with its keys (an
//!   object-literal class) carries the lanes in its birth rep
//!   (`with_lanes`, `codegen::class_birth_reps`): the birth fills them with
//!   the word of 0 and every birth store encodes. A class born keyless
//!   reaches its layout through its field definitions; right after its own
//!   definitions run, before any of its code can read a field,
//!   `js_object_rest_lanes` gives the instance the lanes ([`emit_lanes`]).
//! - **Sites** (`try_lower`, `try_lower_i32`, `try_lower_number`): every
//!   property read, store, update and constant-key `PutValueSet` whose key is
//!   a lane key of some class (`ElisionFacts::rest_keys`) is decided here,
//!   before every other tier: a receiver the facts prove to be one of classes
//!   that all have the lane at their slot loads the slot's low half as an i32
//!   (`load i32`) and stores `INT32_TAG | zext v` (a nullable receiver keeps
//!   its test, whose miss is the complete operation); a receiver proven to be
//!   only of classes without the lane takes its ordinary tiers; any other
//!   site takes the runtime's complete read and store through the layout
//!   accessor (`js_object_rest_get` / `js_object_rest_set`), never a shape or
//!   class guarded raw slot access, since such a tier would read an I32 word
//!   as a JS value.
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`): every proven load first tests that
//! the slot's high half is `INT32_TAG`, every proven store that its value is
//! an int32 Number (a `-0` only for a field PR D tolerates); a miss reports
//! the false proof and exits 86 (`Fact::FieldRep`). Negative control
//! `PERRY_FACTS_SABOTAGE=restlaneraw`: a proven store writes the value's
//! double word, unencoded.

use anyhow::Result;
use perry_hir::Expr;

use super::wpfacts_access::{self, Access, FieldProof};
use super::FnCtx;
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I1, I32, I64};

/// `INT32_TAG` (`0x7FFE_0000_0000_0000`) and its high half.
const INT32_TAG_I64: &str = "9222809086901354496";
const INT32_TAG_HI_I32: &str = "2147352576";

/// The lane word's slots (`perry-runtime` `field_rep::REP_SLOTS`).
const LANE_SLOTS: u32 = 32;

/// The I32 lanes of class `name` (one bit per slot): the fields wpfacts
/// admits (`FactsField::rest`), whose class conditions (no heritage, no
/// computed, private or decorated field, initializers that never observe
/// `this`) wpfacts checks against the declaration, so every module (the
/// defining one and every importer's stub) reads the same lanes. A fact's
/// slot must be the class's own field slot.
pub(crate) fn class_lanes(
    facts: &crate::codegen::ElisionFacts,
    layouts: &crate::layout_families::ProgramLayouts,
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    class_ids: &std::collections::HashMap<String, u32>,
    name: &str,
) -> u32 {
    if facts.rest_keys.is_empty() {
        return 0;
    }
    let (Some(class), Some(cid)) = (classes.get(name), class_ids.get(name)) else {
        return 0;
    };
    let Some(fc) = facts.classes.get(cid) else {
        return 0;
    };
    // A class born with its keys takes its lanes from its birth rep; one
    // born at its static family layout (`layout_families`, planned before
    // the facts) is not, so it has none. A class born keyless gets them
    // after its definitions whatever its birth.
    if layouts.families.contains_key(cid)
        && crate::lower_call::new_alloc::keys_defined_at_birth(class)
    {
        return 0;
    }
    let mut mask = 0u32;
    for (slot, f) in class.fields.iter().enumerate() {
        let slot = slot as u32;
        if slot >= LANE_SLOTS {
            break;
        }
        if fc
            .fields
            .iter()
            .any(|ff| ff.key == f.name && ff.rest && ff.slot == slot)
        {
            mask |= 1 << slot;
        }
    }
    mask
}

/// `rep` with an I32 lane (`11`) at every slot of `mask`.
pub(crate) fn with_lanes(rep: u64, mask: u32) -> u64 {
    let mut rep = rep;
    for slot in 0..LANE_SLOTS {
        if mask & (1 << slot) != 0 {
            rep |= 0b11 << (2 * slot);
        }
    }
    rep
}

/// The I32 lanes in birth rep `rep` (its `11` lanes: a class birth rep has
/// no ConstFn lane).
pub(crate) fn lanes_of_rep(rep: u64) -> u32 {
    (0..LANE_SLOTS)
        .filter(|&s| (rep >> (2 * s)) & 0b11 == 0b11)
        .fold(0, |m, s| m | 1 << s)
}

/// Class `name`'s lanes as the current function sees them (`class_lanes`;
/// a class stub reads its definer's).
pub(crate) fn lanes_of(ctx: &FnCtx<'_>, name: &str) -> u32 {
    class_lanes(
        &ctx.wpfacts,
        &ctx.program_layouts,
        ctx.classes,
        ctx.class_ids,
        name,
    )
}

/// After the field definitions of class `name` (a class born keyless): give
/// the instance `this` its I32 lanes (module docs).
pub(crate) fn emit_lanes(ctx: &mut FnCtx<'_>, name: &str, this: &str) {
    let class = ctx.classes.get(name).copied();
    if class.is_none_or(crate::lower_call::new_alloc::keys_defined_at_birth)
        || !ctx.scalar_ctor_target.is_empty()
    {
        return;
    }
    let mask = lanes_of(ctx, name);
    if mask == 0 {
        return;
    }
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::FieldRep,
        || serde_json::json!({ "rest": "lanes", "class": name, "mask": mask }),
    );
    crate::facts_consumed::used();
    ctx.block().call(
        DOUBLE,
        "js_object_rest_lanes",
        &[(DOUBLE, this), (I32, &(mask as i32).to_string())],
    );
}

/// A fresh, unpublished instance `handle` (an i64) of class `name` born with
/// I32 lanes `lanes`: each lane's slot gets the I32 word of 0 before any
/// store (the shared birth helper fills `undefined`, a non-pointer word the
/// collector skips just as well).
pub(crate) fn emit_birth_fill(ctx: &mut FnCtx<'_>, name: &str, handle: &str, lanes: u32) {
    if lanes == 0 {
        return;
    }
    let base = crate::target_layout::object_slot_base_bytes(
        ctx.target_triple,
        crate::target_layout::class_instance_form(name),
    ) as u64;
    for slot in 0..LANE_SLOTS {
        if lanes & (1 << slot) == 0 {
            continue;
        }
        let blk = ctx.block();
        let addr = blk.add(I64, handle, &(base + 8 * u64::from(slot)).to_string());
        let ptr = blk.inttoptr(I64, &addr);
        // GC_STORE_AUDIT(INIT): the I32 word of 0 in a fresh, unpublished object, never a pointer.
        blk.store(I64, INT32_TAG_I64, &ptr);
    }
}

/// Is `property` a lane key of some class?
fn gated(ctx: &FnCtx<'_>, property: &str) -> bool {
    !ctx.wpfacts.rest_keys.is_empty() && ctx.wpfacts.rest_keys.contains(property)
}

/// Is the read of `property` at `byte_offset` one a lane may reach: a lane
/// key read at a site wpfacts lists (`ElisionFacts::rest_lane_reads`), or
/// at a site without an offset (synthesized, which the facts cannot name)?
fn gated_read(ctx: &FnCtx<'_>, property: &str, byte_offset: u32) -> bool {
    gated(ctx, property)
        && (byte_offset == 0 || ctx.wpfacts.rest_lane_reads.contains_key(&byte_offset))
}

/// The lane slot wpfacts proves for the read at `byte_offset`: every value
/// its receiver may hold is an instance with an I32 lane at that slot for
/// the key, or nullish (`ElisionFacts::rest_lane_reads`).
fn site_slot(ctx: &FnCtx<'_>, byte_offset: u32) -> Option<u32> {
    ctx.wpfacts
        .rest_lane_reads
        .get(&byte_offset)
        .copied()
        .filter(|&s| byte_offset != 0 && s < LANE_SLOTS)
}

/// The receiver is no heap object here: a scalar-replaced binding or the
/// `this` of an inlined scalar constructor, whose fields are locals.
fn scalar(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    match object {
        Expr::LocalGet(id) => {
            ctx.scalar_replaced.contains_key(id) || ctx.pod_records.contains_key(id)
        }
        Expr::This => !ctx.scalar_ctor_target.is_empty(),
        _ => false,
    }
}

/// How a site is lowered.
enum Plan {
    /// The ordinary tiers: every receiver class lacks the lane.
    Ordinary,
    /// The proven slot (every class has the lane at the proof's slot).
    Proven(FieldProof),
    /// The runtime's complete operation through the accessor.
    Complete,
}

fn plan(ctx: &FnCtx<'_>, object: &Expr, property: &str, access: Access) -> Plan {
    let Some((ids, _)) = wpfacts_access::proven_receiver_classes(ctx, object) else {
        return Plan::Complete;
    };
    let lane = |cid: u32, slot: u32| {
        wpfacts_access::class_name_of(ctx, cid)
            .is_some_and(|name| slot < LANE_SLOTS && lanes_of(ctx, &name) & (1 << slot) != 0)
    };
    let with = ids.iter().filter(|&&cid| {
        ctx.wpfacts
            .field(cid, property)
            .is_some_and(|f| lane(cid, f.slot))
    });
    let n = with.count();
    if n == 0 {
        return Plan::Ordinary;
    }
    if n != ids.len() {
        return Plan::Complete;
    }
    match wpfacts_access::prove(ctx, object, property, access) {
        Some(p)
            if !p.guarded()
                && !p.maybe_absent
                && p.class_ids.iter().zip(&p.slots).all(|(&c, &s)| lane(c, s)) =>
        {
            Plan::Proven(p)
        }
        _ => Plan::Complete,
    }
}

/// What a read's consumer takes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Want {
    /// The value, a double.
    Value,
    /// ToInt32 of it, an i32.
    I32,
}

/// The key's handle as a NaN-boxed string.
fn key_box(ctx: &mut FnCtx<'_>, property: &str) -> String {
    let idx = ctx.strings.intern(property);
    let global = format!("@{}", ctx.strings.entry(idx).handle_global);
    ctx.block().load(DOUBLE, &global)
}

/// The complete read of `property` on the boxed `recv`.
fn complete_get(ctx: &mut FnCtx<'_>, recv: &str, property: &str) -> String {
    let key = key_box(ctx, property);
    ctx.block().call(
        DOUBLE,
        "js_object_rest_get",
        &[(DOUBLE, recv), (DOUBLE, &key)],
    )
}

/// The complete store of `value` into `property` of the boxed `recv`.
fn complete_set(
    ctx: &mut FnCtx<'_>,
    recv: &str,
    property: &str,
    value: &str,
    strict: bool,
) -> String {
    let key = key_box(ctx, property);
    let strict = if strict { "1" } else { "0" };
    ctx.block().call(
        DOUBLE,
        "js_object_rest_set",
        &[
            (DOUBLE, recv),
            (DOUBLE, &key),
            (DOUBLE, value),
            (I32, strict),
        ],
    )
}

/// `(hit, miss, merge)` blocks of a nullable receiver's test, the current
/// block left on `hit`; `None` for a receiver the proof says is an object.
fn receiver_test(ctx: &mut FnCtx<'_>, p: &FieldProof, recv: &str) -> Option<(usize, usize)> {
    if !p.nullable {
        return None;
    }
    let hit = ctx.new_block("rest_field.obj");
    let miss = ctx.new_block("rest_field.nullish");
    let (hit_l, miss_l) = (ctx.block_label(hit), ctx.block_label(miss));
    let bits = ctx.block().bitcast_double_to_i64(recv);
    let r = super::receiver_range::emit_fused_receiver_test(ctx.block(), &bits);
    ctx.block().cond_br(&r.is_object_pointer, &hit_l, &miss_l);
    ctx.current_block = hit;
    Some((hit, miss))
}

/// Load the proven slot of `recv` for `want` (module docs).
fn load(ctx: &mut FnCtx<'_>, p: &FieldProof, recv: &str, property: &str, want: Want) -> String {
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::FieldRep,
        || serde_json::json!({ "rest": "read", "key": property, "classes": p.class_ids }),
    );
    crate::facts_consumed::used();
    let ptr = wpfacts_access::emit_proof_slot_ptr(ctx, recv, p);
    if crate::verify_facts::enabled() {
        witness_word(ctx, recv, &ptr, property);
    }
    let v = ctx.block().load(I32, &ptr);
    match want {
        Want::I32 => v,
        Want::Value => ctx.block().sitofp(I32, &v, DOUBLE),
    }
}

/// Store the Number `val` (an int32 by the facts) into the proven slot.
fn store(ctx: &mut FnCtx<'_>, p: &FieldProof, recv: &str, property: &str, val: &str) {
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::FieldRep,
        || serde_json::json!({ "rest": "write", "key": property, "classes": p.class_ids }),
    );
    crate::facts_consumed::used();
    if crate::verify_facts::enabled() {
        witness_value(ctx, val, p.num_zero_ok, property);
    }
    let word = encode(ctx, val);
    let ptr = wpfacts_access::emit_proof_slot_ptr(ctx, recv, p);
    // GC_STORE_AUDIT(POINTER_FREE): an I32 word is in the 0x7FFE band, which no collector takes for a pointer.
    ctx.block().store(I64, &word, &ptr);
}

/// [`encode`] of a value an object-literal birth stores into lane `slot`
/// of `class`, after check mode's witness that it is an int32 Number.
pub(crate) fn encode_birth(ctx: &mut FnCtx<'_>, val: &str, class: &str, slot: usize) -> String {
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::FieldRep,
        || serde_json::json!({ "rest": "birth", "class": class, "slot": slot }),
    );
    crate::facts_consumed::used();
    if crate::verify_facts::enabled() {
        let key = ctx
            .classes
            .get(class)
            .and_then(|c| c.fields.get(slot))
            .map(|f| f.name.clone());
        let zero_ok = match (ctx.class_ids.get(class), key) {
            (Some(&cid), Some(key)) => ctx.wpfacts.zero_tolerant_fields.contains(&(cid, key)),
            _ => false,
        };
        witness_value(ctx, val, zero_ok, &format!("{class}[{slot}]"));
    }
    encode(ctx, val)
}

/// The I32 word of the Number `val` (an int32 by the facts; `-0` is 0).
pub(crate) fn encode(ctx: &mut FnCtx<'_>, val: &str) -> String {
    if sabotaged("restlaneraw") {
        return ctx.block().bitcast_double_to_i64(val);
    }
    let blk = ctx.block();
    let v = blk.fptosi(DOUBLE, val, I32);
    let wide = blk.zext(I32, &v, I64);
    blk.or(I64, &wide, INT32_TAG_I64)
}

/// Lower a gated site `expr` before every other tier (module docs): `None`
/// when `expr` is no such site or its receivers have no lane.
pub(crate) fn try_lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    match expr {
        Expr::PropertyGet {
            object,
            property,
            byte_offset,
            ..
        } if gated_read(ctx, property, *byte_offset) && !scalar(ctx, object) => {
            get(ctx, object, property, *byte_offset, Want::Value)
        }
        Expr::PropertySet {
            object,
            property,
            value,
            ..
        } if gated(ctx, property) && !scalar(ctx, object) => {
            let strict = ctx.is_strict_fn;
            set(ctx, object, property, value, strict)
        }
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            strict,
            ..
        } => match key.as_ref() {
            Expr::String(property)
                if gated(ctx, property) && same_base(target, receiver) && !scalar(ctx, target) =>
            {
                set(ctx, target, property, value, *strict)
            }
            _ => Ok(None),
        },
        Expr::PropertyUpdate {
            object,
            property,
            op,
            prefix,
            ..
        } if gated(ctx, property) && !scalar(ctx, object) => {
            update(ctx, object, property, *op, *prefix)
        }
        _ => Ok(None),
    }
}

/// A `PropertySet` lowered with an explicit strictness (`property_set::
/// lower`, which other lowerings also call).
pub(crate) fn try_lower_set(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
    strict: bool,
) -> Result<Option<String>> {
    match expr {
        Expr::PropertySet {
            object,
            property,
            value,
            ..
        } if gated(ctx, property) && !scalar(ctx, object) => {
            set(ctx, object, property, value, strict)
        }
        _ => Ok(None),
    }
}

/// A `PutValueSet`'s receiver is its target (one evaluation of the base).
fn same_base(target: &Expr, receiver: &Expr) -> bool {
    matches!((target, receiver), (Expr::This, Expr::This))
        || matches!((target, receiver), (Expr::LocalGet(a), Expr::LocalGet(b)) if a == b)
}

/// An int consumer's read of a gated site: the i32.
pub(crate) fn try_lower_i32(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    match expr {
        Expr::PropertyGet {
            object,
            property,
            byte_offset,
            ..
        } if gated_read(ctx, property, *byte_offset) && !scalar(ctx, object) => {
            get(ctx, object, property, *byte_offset, Want::I32)
        }
        _ => Ok(None),
    }
}

/// A number context's read of a gated site whose receivers have the lane:
/// the canonical double of the proven slot. `None` for every other site
/// (whose ordinary or complete lowering `lower_expr` reaches).
pub(crate) fn try_lower_number(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    match expr {
        Expr::PropertyGet {
            object,
            property,
            byte_offset,
            ..
        } if gated_read(ctx, property, *byte_offset) && !scalar(ctx, object) => {
            match plan(ctx, object, property, Access::Get) {
                Plan::Proven(p) if !p.nullable => {
                    let recv = super::lower_expr(ctx, object)?;
                    Ok(Some(load(ctx, &p, &recv, property, Want::Value)))
                }
                _ => Ok(None),
            }
        }
        _ => Ok(None),
    }
}

/// Is `expr` a gated read, which a raw slot tier must leave alone?
pub(crate) fn is_gated_read(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    matches!(expr, Expr::PropertyGet { object, property, byte_offset, .. }
        if gated_read(ctx, property, *byte_offset) && !scalar(ctx, object))
}

fn get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    byte_offset: u32,
    want: Want,
) -> Result<Option<String>> {
    let p = match plan(ctx, object, property, Access::Get) {
        Plan::Ordinary => return Ok(None),
        Plan::Proven(p) => p,
        Plan::Complete if site_slot(ctx, byte_offset).is_some() => {
            let slot = site_slot(ctx, byte_offset).unwrap_or_default();
            return site_get(ctx, object, property, slot, want).map(Some);
        }
        Plan::Complete if class_guard(ctx, property).is_some() => {
            let (cids, slot) = class_guard(ctx, property).unwrap_or_default();
            return guarded_get(ctx, object, property, &cids, slot, want).map(Some);
        }
        Plan::Complete => {
            super::note_site(&format!(
                "rest-field-complete-get .{} {}",
                property, ctx.func.name
            ));
            let recv = super::lower_expr(ctx, object)?;
            let v = complete_get(ctx, &recv, property);
            return Ok(Some(match want {
                Want::Value => v,
                Want::I32 => {
                    let n = ctx
                        .block()
                        .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &v)]);
                    ctx.toint32_wrap(&n)
                }
            }));
        }
    };
    super::note_site(&format!("rest-field-get .{} {}", property, ctx.func.name));
    let recv = super::lower_expr(ctx, object)?;
    let Some((hit, miss)) = receiver_test(ctx, &p, &recv) else {
        return Ok(Some(load(ctx, &p, &recv, property, want)));
    };
    let fast = load(ctx, &p, &recv, property, want);
    let fast_end = ctx.block().label.clone();
    let merge = ctx.new_block("rest_field.read");
    let merge_l = ctx.block_label(merge);
    ctx.block().br(&merge_l);
    let _ = hit;
    ctx.current_block = miss;
    // The complete read throws for a nullish receiver.
    let slow = complete_get(ctx, &recv, property);
    let slow = match want {
        Want::Value => slow,
        Want::I32 => {
            let n = ctx
                .block()
                .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &slow)]);
            ctx.toint32_wrap(&n)
        }
    };
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = merge;
    let ty = if want == Want::I32 { I32 } else { DOUBLE };
    Ok(Some(
        ctx.block()
            .phi(ty, &[(&fast, &fast_end), (&slow, &slow_end)]),
    ))
}

/// The raw I32 word of a proven, non-nullish read of a lane (an `i64`):
/// what a store into another lane copies as it is (`set`, and an
/// object-literal birth's argument, `lower_call::new`). `None` for any other
/// expression.
pub(crate) fn try_lower_word(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    let Expr::PropertyGet {
        object,
        property,
        byte_offset,
        ..
    } = expr
    else {
        return Ok(None);
    };
    if !gated_read(ctx, property, *byte_offset) || scalar(ctx, object) || !binding(object) {
        return Ok(None);
    }
    let Plan::Proven(p) = plan(ctx, object, property, Access::Get) else {
        return Ok(None);
    };
    if p.nullable {
        return Ok(None);
    }
    super::note_site(&format!("rest-field-word .{} {}", property, ctx.func.name));
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::FieldRep,
        || serde_json::json!({ "rest": "word", "key": property, "classes": p.class_ids }),
    );
    crate::facts_consumed::used();
    let recv = super::lower_expr(ctx, object)?;
    let ptr = wpfacts_access::emit_proof_slot_ptr(ctx, &recv, &p);
    if crate::verify_facts::enabled() {
        witness_word(ctx, &recv, &ptr, property);
    }
    Ok(Some(ctx.block().load(I64, &ptr)))
}

/// A receiver whose evaluation reads one binding: no side effect, no
/// allocation, so a store may evaluate its value after it unrooted.
fn binding(e: &Expr) -> bool {
    matches!(e, Expr::This | Expr::LocalGet(_))
}

/// The classes that keep `property` in an I32 lane, when every one is a
/// class (not an object-literal class, whose instances carry no class id)
/// and all hold it at one slot: a receiver whose class id is one of them has
/// the lane there (an admitted class has no heritage, and its instances get
/// their lanes before any of its code runs).
fn class_guard(ctx: &FnCtx<'_>, property: &str) -> Option<(Vec<u32>, u32)> {
    let mut cids = Vec::new();
    let mut slot: Option<u32> = None;
    for (&cid, fc) in &ctx.wpfacts.classes {
        let Some(f) = fc.fields.iter().find(|f| f.key == property && f.rest) else {
            continue;
        };
        let name = wpfacts_access::class_name_of(ctx, cid)?;
        let class = ctx.classes.get(&name)?;
        if class.is_literal_shape() || lanes_of(ctx, &name) & (1 << f.slot) == 0 {
            return None;
        }
        if slot.is_some_and(|s| s != f.slot) {
            return None;
        }
        slot = Some(f.slot);
        cids.push(cid);
    }
    Some((cids, slot?)).filter(|(c, s)| !c.is_empty() && *s < LANE_SLOTS)
}

/// The class-id test of a guarded lane access: the current block branches to
/// the returned `(hit, miss)` blocks, `hit` holding the slot pointer.
fn guard(ctx: &mut FnCtx<'_>, recv: &str, cids: &[u32], slot: u32) -> (usize, usize, String) {
    let chk = ctx.new_block("rest_field.guard_cid");
    let hit = ctx.new_block("rest_field.guard_hit");
    let miss = ctx.new_block("rest_field.guard_miss");
    let (chk_l, hit_l, miss_l) = (
        ctx.block_label(chk),
        ctx.block_label(hit),
        ctx.block_label(miss),
    );
    let bits = ctx.block().bitcast_double_to_i64(recv);
    let r = super::receiver_range::emit_fused_receiver_test(ctx.block(), &bits);
    ctx.block().cond_br(&r.is_object_pointer, &chk_l, &miss_l);
    ctx.current_block = chk;
    let blk = ctx.block();
    // Word 0 is a class id only on a GC_TYPE_OBJECT.
    let type_ptr =
        super::receiver_range::emit_field_ptr(blk, &r.biased, -(perry_abi::GC_HEADER_SIZE as i64));
    let type_byte = blk.load(crate::types::I8, &type_ptr);
    let is_object = crate::target_layout::emit_gc_type_is_object(blk, crate::types::I8, &type_byte);
    let hdr = super::receiver_range::emit_field_ptr(blk, &r.biased, 0);
    let cid = blk.load(I32, &hdr);
    let mut any: Option<String> = None;
    for c in cids {
        let eq = blk.icmp_eq(I32, &cid, &c.to_string());
        any = Some(match any {
            Some(prev) => blk.or(I1, &prev, &eq),
            None => eq,
        });
    }
    let ok = blk.and(I1, &is_object, &any.unwrap_or_else(|| "false".into()));
    blk.cond_br(&ok, &hit_l, &miss_l);
    ctx.current_block = hit;
    let form = crate::target_layout::HeaderForm::Meta;
    let ptr = wpfacts_access::emit_slot_ptr(ctx, recv, form, slot);
    (hit, miss, ptr)
}

/// A read guarded by the receiver's class id (`class_guard`).
fn guarded_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    cids: &[u32],
    slot: u32,
    want: Want,
) -> Result<String> {
    super::note_site(&format!(
        "rest-field-guarded-get .{} {}",
        property, ctx.func.name
    ));
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::FieldRep,
        || serde_json::json!({ "rest": "guarded", "key": property, "classes": cids }),
    );
    crate::facts_consumed::used();
    let recv = super::lower_expr(ctx, object)?;
    let (_, miss, ptr) = guard(ctx, &recv, cids, slot);
    if crate::verify_facts::enabled() {
        witness_word(ctx, &recv, &ptr, property);
    }
    let v = ctx.block().load(I32, &ptr);
    let fast = match want {
        Want::I32 => v,
        Want::Value => ctx.block().sitofp(I32, &v, DOUBLE),
    };
    let fast_end = ctx.block().label.clone();
    let merge = ctx.new_block("rest_field.guard_read");
    let merge_l = ctx.block_label(merge);
    ctx.block().br(&merge_l);
    ctx.current_block = miss;
    let slow = complete_get(ctx, &recv, property);
    let slow = match want {
        Want::Value => slow,
        Want::I32 => {
            let n = ctx
                .block()
                .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &slow)]);
            ctx.toint32_wrap(&n)
        }
    };
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = merge;
    let ty = if want == Want::I32 { I32 } else { DOUBLE };
    Ok(ctx
        .block()
        .phi(ty, &[(&fast, &fast_end), (&slow, &slow_end)]))
}

/// A read wpfacts proves by its site (`site_slot`): behind the receiver's
/// nullish test, the i32 of lane `slot`; a nullish receiver takes the
/// complete read, which throws.
fn site_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    slot: u32,
    want: Want,
) -> Result<String> {
    super::note_site(&format!(
        "rest-field-site-get .{} {}",
        property, ctx.func.name
    ));
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::FieldRep,
        || serde_json::json!({ "rest": "site", "key": property, "slot": slot }),
    );
    crate::facts_consumed::used();
    let recv = super::lower_expr(ctx, object)?;
    let hit = ctx.new_block("rest_field.site_obj");
    let miss = ctx.new_block("rest_field.site_nullish");
    let merge = ctx.new_block("rest_field.site_read");
    let (hit_l, miss_l, merge_l) = (
        ctx.block_label(hit),
        ctx.block_label(miss),
        ctx.block_label(merge),
    );
    let bits = ctx.block().bitcast_double_to_i64(&recv);
    let r = super::receiver_range::emit_fused_receiver_test(ctx.block(), &bits);
    ctx.block().cond_br(&r.is_object_pointer, &hit_l, &miss_l);
    ctx.current_block = hit;
    let form = crate::target_layout::HeaderForm::Meta;
    let ptr = wpfacts_access::emit_slot_ptr(ctx, &recv, form, slot);
    if crate::verify_facts::enabled() {
        witness_word(ctx, &recv, &ptr, property);
    }
    let v = ctx.block().load(I32, &ptr);
    let fast = match want {
        Want::I32 => v,
        Want::Value => ctx.block().sitofp(I32, &v, DOUBLE),
    };
    let fast_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = miss;
    let slow = complete_get(ctx, &recv, property);
    let slow = match want {
        Want::Value => slow,
        Want::I32 => {
            let n = ctx
                .block()
                .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &slow)]);
            ctx.toint32_wrap(&n)
        }
    };
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = merge;
    let ty = if want == Want::I32 { I32 } else { DOUBLE };
    Ok(ctx
        .block()
        .phi(ty, &[(&fast, &fast_end), (&slow, &slow_end)]))
}

fn set(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    value: &Expr,
    strict: bool,
) -> Result<Option<String>> {
    let p = match plan(ctx, object, property, Access::Set) {
        Plan::Ordinary => return Ok(None),
        Plan::Proven(p) => Some(p),
        Plan::Complete => None,
    };
    // Lane to lane: the source's I32 word is the stored word (no decode, no
    // encode). Both receivers are bindings, so nothing runs between them.
    if let Some(dst) = p.as_ref().filter(|p| !p.nullable && binding(object)) {
        if let Expr::PropertyGet { object: src, .. } = value {
            if binding(src) {
                let recv = super::lower_expr(ctx, object)?;
                if let Some(word) = try_lower_word(ctx, value)? {
                    super::note_site(&format!("rest-field-copy .{} {}", property, ctx.func.name));
                    crate::verify_facts::consumed(
                        crate::verify_facts::Fact::FieldRep,
                        || serde_json::json!({ "rest": "copy", "key": property }),
                    );
                    let ptr = wpfacts_access::emit_proof_slot_ptr(ctx, &recv, dst);
                    // GC_STORE_AUDIT(POINTER_FREE): an I32 word (0x7FFE band), never a pointer.
                    ctx.block().store(I64, &word, &ptr);
                    let low = ctx.block().trunc(I64, &word, I32);
                    return Ok(Some(ctx.block().sitofp(I32, &low, DOUBLE)));
                }
            }
        }
    }
    let property = property.to_string();
    crate::rooting::with_operands_rooted(ctx, &[object, value], |ctx, v| {
        let (recv, val) = (&v[0], &v[1]);
        let Some(p) = &p else {
            if let Some((cids, slot)) = class_guard(ctx, &property) {
                super::note_site(&format!(
                    "rest-field-guarded-set .{} {}",
                    &property, ctx.func.name
                ));
                let (_, miss, ptr) = guard(ctx, recv, &cids, slot);
                if crate::verify_facts::enabled() {
                    witness_value(ctx, val, true, &property);
                }
                let word = encode(ctx, val);
                // GC_STORE_AUDIT(POINTER_FREE): an I32 word (0x7FFE band), never a pointer.
                ctx.block().store(I64, &word, &ptr);
                let merge = ctx.new_block("rest_field.guard_stored");
                let merge_l = ctx.block_label(merge);
                ctx.block().br(&merge_l);
                ctx.current_block = miss;
                complete_set(ctx, recv, &property, val, strict);
                ctx.block().br(&merge_l);
                ctx.current_block = merge;
                return Ok(Some(val.clone()));
            }
            super::note_site(&format!(
                "rest-field-complete-set .{} {}",
                &property, ctx.func.name
            ));
            complete_set(ctx, recv, &property, val, strict);
            return Ok(Some(val.clone()));
        };
        super::note_site(&format!("rest-field-set .{} {}", &property, ctx.func.name));
        let Some((_, miss)) = receiver_test(ctx, p, recv) else {
            store(ctx, p, recv, &property, val);
            return Ok(Some(val.clone()));
        };
        store(ctx, p, recv, &property, val);
        let merge = ctx.new_block("rest_field.stored");
        let merge_l = ctx.block_label(merge);
        ctx.block().br(&merge_l);
        ctx.current_block = miss;
        complete_set(ctx, recv, &property, val, strict);
        ctx.block().br(&merge_l);
        ctx.current_block = merge;
        Ok(Some(val.clone()))
    })
}

/// `o.k++` / `o.k--` (prefix or postfix).
fn update(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    op: perry_hir::BinaryOp,
    prefix: bool,
) -> Result<Option<String>> {
    let p = match plan(ctx, object, property, Access::Update) {
        Plan::Ordinary => return Ok(None),
        Plan::Proven(p) if !p.nullable => Some(p),
        _ => None,
    };
    let delta = if matches!(op, perry_hir::BinaryOp::Sub) {
        -1.0
    } else {
        1.0
    };
    let recv = super::lower_expr(ctx, object)?;
    let (old, new) = match &p {
        Some(p) => {
            super::note_site(&format!(
                "rest-field-update .{} {}",
                property, ctx.func.name
            ));
            let old = load(ctx, p, &recv, property, Want::Value);
            let new = ctx.block().fadd(&old, &double_literal(delta));
            store(ctx, p, &recv, property, &new);
            (old, new)
        }
        None => {
            // The complete read, ToNumeric, the step, the complete store: the
            // receiver is held across the two calls, which may collect.
            let mut group = crate::rooting::open_rooted_group(3);
            let owner = group.adopt_emitted(ctx, crate::rooting::Repr::Boxed, &recv, true);
            let got = complete_get(ctx, &recv, property);
            let old = ctx.block().call(DOUBLE, "js_to_numeric", &[(DOUBLE, &got)]);
            let old_root = group.adopt_emitted(ctx, crate::rooting::Repr::Boxed, &old, true);
            let inc = if delta > 0.0 { "1" } else { "0" };
            let new = ctx
                .block()
                .call(DOUBLE, "js_numeric_step", &[(DOUBLE, &old), (I32, inc)]);
            let new_root = group.adopt_emitted(ctx, crate::rooting::Repr::Boxed, &new, true);
            let recv = group.reread_emitted(ctx, owner);
            let new = group.reread_emitted(ctx, new_root);
            let strict = ctx.is_strict_fn;
            complete_set(ctx, &recv, property, &new, strict);
            let old = group.reread_emitted(ctx, old_root);
            let new = group.reread_emitted(ctx, new_root);
            group.release(ctx);
            (old, new)
        }
    };
    Ok(Some(if prefix { new } else { old }))
}

/// The negative control `PERRY_FACTS_SABOTAGE=<arm>`.
fn sabotaged(arm: &str) -> bool {
    std::env::var("PERRY_FACTS_SABOTAGE").as_deref() == Ok(arm)
}

/// Check mode: the slot at `ptr` holds an I32 word.
fn witness_word(ctx: &mut FnCtx<'_>, recv: &str, ptr: &str, property: &str) {
    crate::verify_facts::covered(
        crate::verify_facts::Fact::FieldRep,
        || serde_json::json!({ "rest": "word", "key": property }),
    );
    let raw = ctx.block().load(I64, ptr);
    let hi = ctx.block().lshr(I64, &raw, "32");
    let hi = ctx.block().trunc(I64, &hi, I32);
    let ok = ctx.block().icmp_eq(I32, &hi, INT32_TAG_HI_I32);
    miss_unless(
        ctx,
        &ok,
        recv,
        &format!("rest lane: the I32 slot of .{property} holds no I32 word"),
    );
}

/// Check mode: the stored `val` is an int32 Number (`-0` only when
/// `zero_ok`).
fn witness_value(ctx: &mut FnCtx<'_>, val: &str, zero_ok: bool, property: &str) {
    crate::verify_facts::covered(
        crate::verify_facts::Fact::FieldRep,
        || serde_json::json!({ "rest": "value", "key": property }),
    );
    let blk = ctx.block();
    let ge = blk.fcmp("oge", val, "-2147483648.0");
    let le = blk.fcmp("ole", val, "2147483647.0");
    let in_range = blk.and(I1, &ge, &le);
    let safe = blk.select(I1, &in_range, DOUBLE, val, "0.5");
    let int = blk.fptosi(DOUBLE, &safe, I32);
    let back = blk.sitofp(I32, &int, DOUBLE);
    let exact = blk.fcmp("oeq", &back, val);
    let ok = if zero_ok {
        exact
    } else {
        let bits = blk.bitcast_double_to_i64(val);
        let not_negative_zero = blk.icmp_ne(I64, &bits, "-9223372036854775808");
        blk.and(I1, &exact, &not_negative_zero)
    };
    miss_unless(
        ctx,
        &ok,
        val,
        &format!("rest lane: a store of a value that is no int32 into .{property}"),
    );
}

/// Report a false proof (exit 86) unless `ok`.
fn miss_unless(ctx: &mut FnCtx<'_>, ok: &str, value: &str, what: &str) {
    crate::verify_facts::tick(ctx);
    let miss = ctx.new_block("rest_field.verify_miss");
    let next = ctx.new_block("rest_field.verify_ok");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    ctx.block().cond_br(ok, &next_l, &miss_l);
    ctx.current_block = miss;
    let site = crate::verify_facts::site_arg(ctx, what, crate::verify_facts::Fact::FieldRep, &[]);
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, value)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
}
