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
//! `this_of`).
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
//! elided site goes to `<dump>.elisions`; in a `PERRY_CHECK_CONTRACTS=1`
//! build each elided site calls `perry_wpfacts_check_field` (perry-runtime
//! `wpfacts_check.rs`), which re-derives class, slot and kind from the
//! object and aborts on a mismatch.

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
}

impl FieldProof {
    /// Every class holds the field at one slot (§3.7 case 1).
    pub(crate) fn agrees(&self) -> bool {
        self.slots.iter().all(|s| *s == self.slot)
    }

    /// The site tests the proof at run time and has a generic miss arm.
    pub(crate) fn guarded(&self) -> bool {
        self.checked || self.interp_guard
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
            ctx.wpfacts
                .locals
                .get(id)
                .map(|r| (r.clone(), format!("local {id}")))
        }
        Expr::This => {
            if ctx.is_static_class_this(object) {
                return None;
            }
            let top = ctx.this_stack.last()?;
            let (_, fid) = ctx.wpfacts_this.iter().rev().find(|(s, _)| s == top)?;
            ctx.wpfacts
                .this_of
                .get(fid)
                .map(|r| (r.clone(), format!("this of fn {fid}")))
        }
        _ => None,
    }
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

fn tested_in_block(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    stable_binding(ctx, object).is_some_and(|b| {
        ctx.wpfacts_tested
            .iter()
            .any(|&(t, blk)| t == b && blk == ctx.current_block)
    })
}

/// A number-context read (`lower_raw_f64_class_field_get_for_number_context`,
/// `expr_produces_canonical_raw_f64`): the facts prove the slot always holds
/// a canonical double.
pub(crate) fn proves_raw_f64(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    let Expr::PropertyGet {
        object, property, ..
    } = expr
    else {
        return false;
    };
    prove(ctx, object, property, Access::Get).is_some_and(|p| p.raw_f64)
}

/// The local class of class id `cid` (its own name, not an alias).
fn class_name_of(ctx: &FnCtx<'_>, cid: u32) -> Option<String> {
    ctx.classes
        .iter()
        .find(|(name, c)| c.id == cid && c.name == **name)
        .map(|(name, _)| name.clone())
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
    if recv.class_ids.is_empty() || recv.class_ids.len() > 4 {
        crate::fallback_diag::refuse("receiver class set is empty or exceeds four classes");
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
    if recv.nullable && access != Access::Get {
        crate::fallback_diag::refuse(
            "nullable store/update receiver requires generic nullish semantics",
        );
        return None;
    }
    let mut slot: Option<u32> = None;
    let mut kind_mask = 0u16;
    let mut ta_stable = true;
    let mut ta_kind: Option<Option<u8>> = None;
    let mut all_f64 = true;
    let mut any_f64 = false;
    let mut names = Vec::new();
    let mut slots = Vec::new();
    for &cid in &recv.class_ids {
        let name = class_name_of(ctx, cid).or_else(|| {
            crate::fallback_diag::refuse("receiver class is absent from codegen class metadata");
            None
        })?;
        let f = ctx.wpfacts.field(cid, property).or_else(|| {
            crate::fallback_diag::refuse(
                "key is not a definite data birth field of every receiver class",
            );
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
        if super::property_set::class_has_computed_runtime_members(ctx, &name)
            || super::class_field_inline_guard::class_instances_carry_private_elements(ctx, &name)
            || super::class_field_inline_guard::class_instances_grow_past_layout(ctx, &name)
        {
            crate::fallback_diag::refuse(
                "computed/private/growing class layout refuses fixed-offset field access",
            );
            return None;
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
    let guarded = checked || interp_guard;
    Some(FieldProof {
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
        any_f64_lane: any_f64,
        binding,
        checked,
        interp_guard,
    })
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
        "E {site:#018x} {what} fn={} .{property} recv={} classes={:?} slot={} nullable={} kind_mask={:#x} raw_f64={} proof={} facts={}\n",
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
    if !crate::codegen::helpers::check_contracts() {
        return;
    }
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let (c01, c23) = pack_cids(&p.class_ids);
    let slot = if p.agrees() {
        p.slot.to_string()
    } else {
        let bits = ctx.block().bitcast_double_to_i64(recv_box);
        let biased = ctx.block().sub(
            I64,
            &bits,
            &(super::receiver_range::RECEIVER_BIAS as i64).to_string(),
        );
        select_by_class(ctx, &biased, p, |s| s.to_string())
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
            (I32, &(p.kind_mask as i32).to_string()),
            (I32, &flags.to_string()),
            (I64, &(site as i64).to_string()),
        ],
    );
}

/// The slot pointer of a receiver known to be an object, addressed from the
/// receiver's biased value (`receiver_range`): the handle floor and the slot
/// offset fold into the load's displacement. (Masking the NaN-box with
/// `POINTER_MASK` instead lowers to `bzhi` with an 8-bit count register on
/// BMI2 targets, whose partial-register write makes the load wait on an
/// unrelated earlier value: binary-trees arena +8% cycles.)
pub(crate) fn emit_slot_ptr(ctx: &mut FnCtx<'_>, recv_box: &str, slot: u32) -> String {
    let header = crate::target_layout::object_header_size_bytes(ctx.target_triple) as i64;
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
        return emit_slot_ptr(ctx, recv_box, p.slot);
    }
    let header = crate::target_layout::object_header_size_bytes(ctx.target_triple) as i64;
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
        val
    };
    let numeric = p.raw_f64;
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
            let is_object = blk.icmp_eq(I8, &type_byte, &perry_abi::GC_TYPE_OBJECT.to_string());
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
    let fast_end = ctx.block().label.clone();
    ctx.block().br(&done_label);
    ctx.current_block = miss_idx;
    let generic = Expr::PropertyGet {
        object: Box::new(object.clone()),
        property: property.to_string(),
        byte_offset,
    };
    let was_off = std::mem::replace(&mut ctx.wpfacts_off, true);
    let slow = lower_expr(ctx, &generic);
    ctx.wpfacts_off = was_off;
    let slow = slow?;
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&done_label);
    ctx.current_block = done_idx;
    let value = ctx
        .block()
        .phi(DOUBLE, &[(&fast, &fast_end), (&slow, &slow_end)]);
    record(ctx, site, "get", property, &p);
    Ok(value)
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
