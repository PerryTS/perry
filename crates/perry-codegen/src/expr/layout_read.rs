//! Object model step 5, case 2c (OBJECT_MODEL_2026-10-13.md §3.7): the read
//! of a coloured key through a receiver nothing proves.
//!
//! A coloured key `k` (`layout_families`) has one program-wide slot, and
//! every layout record carries a mask of the coloured keys its receivers hold
//! as inline data at their colour slot (perry-runtime `object/layout_probe.rs`
//! derives it from the record's immutable key list). The site is:
//!
//! ```text
//! tag test -> layout word -> record (one address computation) -> mask bit
//!          -> one load at the colour slot -> hole compare
//! ```
//!
//! * Masking the layout word with LAYOUT_PROBE_INDEX_MASK and subtracting
//!   LAYOUT_TAG produces its row in the agent's dense record Region. The mask
//!   clears shadow and prototype kind, retaining tag/band for one admission
//!   against the logical issued-row bound in the existing directory.
//!   A live ordinary receiver word is the publication authority.
//! * The mask bit says the layout holds `k` at its colour slot as data; the
//!   slot's value is the property's value unless it is the hole marker.
//! * Everything else (a non-object, another layout, the bit clear, a hole)
//!   takes the site's ordinary path: the unknown-receiver probe, then the
//!   complete slow operation.
//!
//! No state is learned or validated: the word, the record and the mask are
//! the object's own data and immutable layout facts.

use crate::nanbox::TAG_HOLE_I64;
use crate::types::{DOUBLE, I32, I64, I8, PTR};
use perry_hir::Expr;

use super::FnCtx;

/// An emitted 2c arm: its hit value and the block it was produced in. The
/// current block, on return from [`begin`], is the miss path.
pub(crate) struct LayoutRead {
    hit_value: String,
    hit_label: String,
    merge_idx: usize,
}

/// The (mask bit, slot) when `property` is read through the 2c arm.
pub(crate) fn admitted(ctx: &FnCtx<'_>, property: &str) -> Option<(u32, u32)> {
    if property == "length" || property == "size" || property.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if crate::target_layout::target_is_ilp32(ctx.target_triple)
        || ctx.target_triple.contains("wasm")
    {
        return None;
    }
    ctx.program_layouts.colored(property)
}

/// The record mask word of `bit` for the receiver whose biased payload is
/// `biased`, in the current block, branching to `miss_label` when the
/// receiver's layout word names no record. Leaves the current block at the
/// mask test's true edge and returns the loaded mask word's test result.
fn emit_mask_test(
    ctx: &mut FnCtx<'_>,
    biased: &str,
    bit: u32,
    miss_label: &str,
    mask_offset: usize,
) -> String {
    emit_mask_test_record(ctx, biased, bit, miss_label, mask_offset).0
}

/// [`emit_mask_test`], also returning the record's address (valid in the
/// block it leaves).
fn emit_mask_test_record(
    ctx: &mut FnCtx<'_>,
    biased: &str,
    bit: u32,
    miss_label: &str,
    mask_offset: usize,
) -> (String, String) {
    let blk = ctx.block();
    let word_ptr = crate::expr::receiver_range::emit_field_ptr(blk, biased, 4);
    let word = blk.load(I32, &word_ptr);
    let id = blk.and(
        I32,
        &word,
        &(crate::runtime_abi::LAYOUT_PROBE_INDEX_MASK as i32).to_string(),
    );
    let rel = blk.sub(
        I32,
        &id,
        &(crate::runtime_abi::LAYOUT_TAG as i32).to_string(),
    );
    let rel64 = blk.zext(I32, &rel, I64);
    let dir = crate::expr::agent_ptr::emit_agent_ptr_or(
        ctx,
        crate::runtime_abi::AGENT_PTR_SHAPE_DIR,
        "@PERRY_EMPTY_SHAPE_DIR",
    );
    let blk = ctx.block();
    let base_ptr = blk.gep(
        I8,
        &dir,
        &[(I64, &crate::runtime_abi::SHAPE_DIR_FLAT_BASE.to_string())],
    );
    let base = blk.load(PTR, &base_ptr);
    let ids_ptr = blk.gep(
        I8,
        &dir,
        &[(
            I64,
            &(crate::runtime_abi::SHAPE_DIR_FLAT_BASE + 8).to_string(),
        )],
    );
    let ids = blk.load(I64, &ids_ptr);
    let inside = blk.icmp_ult(I64, &rel64, &ids);
    let rec_idx = ctx.new_block("l2c.record");
    let rec_label = ctx.block_label(rec_idx);
    ctx.block().cond_br(&inside, &rec_label, miss_label);
    ctx.current_block = rec_idx;
    let blk = ctx.block();
    let offset = blk.mul(
        I64,
        &rel64,
        &crate::runtime_abi::SHAPE_RECORD_SIZE.to_string(),
    );
    let rec = blk.gep(I8, &base, &[(I64, &offset)]);
    let mask_at = blk.gep(
        I8,
        &rec,
        &[(I64, &(mask_offset + 8 * (bit / 64) as usize).to_string())],
    );
    let mask_word = blk.load(I64, &mask_at);
    let bit_value = blk.and(I64, &mask_word, &((1u64 << (bit % 64)) as i64).to_string());
    (blk.icmp_ne(I64, &bit_value, "0"), rec)
}

/// Emit the 2c arm for `receiver_bits.property` if the key is coloured.
/// On `Some`, the caller lowers its ordinary site in the current block (the
/// miss path) and passes its value to [`finish`].
pub(crate) fn begin(
    ctx: &mut FnCtx<'_>,
    receiver_bits: &str,
    property: &str,
) -> Option<LayoutRead> {
    let (bit, slot) = admitted(ctx, property)?;
    crate::expr::note_site("case2c");
    let header =
        crate::target_layout::object_slot_base_bytes(ctx.target_triple, ctx.program_layouts.form());
    let word_idx = ctx.new_block("l2c.word");
    let load_idx = ctx.new_block("l2c.load");
    let hit_idx = ctx.new_block("l2c.hit");
    let miss_idx = ctx.new_block("l2c.miss");
    let merge_idx = ctx.new_block("l2c.merge");
    let word_label = ctx.block_label(word_idx);
    let load_label = ctx.block_label(load_idx);
    let hit_label = ctx.block_label(hit_idx);
    let miss_label = ctx.block_label(miss_idx);

    let recv = crate::expr::receiver_range::emit_fused_receiver_test(ctx.block(), receiver_bits);
    ctx.block()
        .cond_br(&recv.is_object_pointer, &word_label, &miss_label);

    ctx.current_block = word_idx;
    let holds = emit_mask_test(
        ctx,
        &recv.biased,
        bit,
        &miss_label,
        crate::runtime_abi::SHAPE_RECORD_COLOUR_MASK,
    );
    ctx.block().cond_br(&holds, &load_label, &miss_label);

    ctx.current_block = load_idx;
    let blk = ctx.block();
    let value_ptr = crate::expr::receiver_range::emit_field_ptr(
        blk,
        &recv.biased,
        (header + 8 * slot as u64) as i64,
    );
    let value = blk.load(I64, &value_ptr);
    let hole = blk.icmp_eq(I64, &value, TAG_HOLE_I64);
    blk.cond_br(&hole, &miss_label, &hit_label);

    ctx.current_block = hit_idx;
    let merge_label = ctx.block_label(merge_idx);
    let blk = ctx.block();
    let hit_value = blk.bitcast_i64_to_double(&value);
    blk.br(&merge_label);
    let hit_label_now = ctx.block_label(ctx.current_block);

    ctx.current_block = miss_idx;
    Some(LayoutRead {
        hit_value,
        hit_label: hit_label_now,
        merge_idx,
    })
}

/// Join the 2c hit with the ordinary site's value `generic` (produced in the
/// current block).
pub(crate) fn finish(ctx: &mut FnCtx<'_>, arm: LayoutRead, generic: &str) -> String {
    let merge_label = ctx.block_label(arm.merge_idx);
    let generic_label = ctx.block_label(ctx.current_block);
    ctx.block().br(&merge_label);
    ctx.current_block = arm.merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (arm.hit_value.as_str(), arm.hit_label.as_str()),
            (generic, generic_label.as_str()),
        ],
    )
}

/// An emitted 2c store arm (see [`begin_store`]).
pub(crate) struct LayoutStore {
    hit_label: String,
    merge_idx: usize,
}

/// The store form of the 2c site for a coloured key: the receiver's layout
/// holds `property` at its colour slot as a plain writable `Any`-lane data
/// property of a store-admitted layout (the record's store mask), the object
/// carries no metadata record (no prototype role, no exotic receiver), and
/// the slot is present; then the assignment is one store plus the write
/// barrier's bookkeeping. On `Some`, the caller lowers its ordinary store
/// site in the current block (the miss path) and passes its value to
/// [`finish_store`].
pub(crate) fn begin_store(
    ctx: &mut FnCtx<'_>,
    receiver_bits: &str,
    property: &str,
    value_double: &str,
    value_bits: &str,
    value_is_scalar: bool,
) -> Option<LayoutStore> {
    let (bit, slot) = admitted(ctx, property)?;
    crate::expr::note_site("case2c-store");
    let header =
        crate::target_layout::object_slot_base_bytes(ctx.target_triple, ctx.program_layouts.form());
    let word_idx = ctx.new_block("l2s.word");
    let meta_idx = ctx.new_block("l2s_meta");
    let load_idx = ctx.new_block("l2s.load");
    let hit_idx = ctx.new_block("l2s.hit");
    let miss_idx = ctx.new_block("l2s.miss");
    let merge_idx = ctx.new_block("l2s.merge");
    let word_label = ctx.block_label(word_idx);
    let meta_label = ctx.block_label(meta_idx);
    let load_label = ctx.block_label(load_idx);
    let hit_label = ctx.block_label(hit_idx);
    let miss_label = ctx.block_label(miss_idx);
    let merge_label = ctx.block_label(merge_idx);

    let recv = crate::expr::receiver_range::emit_fused_receiver_test(ctx.block(), receiver_bits);
    ctx.block()
        .cond_br(&recv.is_object_pointer, &word_label, &miss_label);

    ctx.current_block = word_idx;
    let (holds, rec) = emit_mask_test_record(
        ctx,
        &recv.biased,
        bit,
        &miss_label,
        crate::runtime_abi::SHAPE_RECORD_COLOUR_STORE_MASK,
    );
    if slot >= crate::runtime_abi::SHAPE_REP_SLOTS {
        // A slot past the lane word is `Any`: the mask is the whole proof.
        ctx.block().cond_br(&holds, &meta_label, &miss_label);
    } else {
        // The mask admits an `Any` or an `F64` lane. A plain double fits
        // either; anything else needs the record's lane for this slot to be
        // `Any` (`rep` bits 2*slot..2*slot+1 are zero).
        let lane_idx = ctx.new_block("l2s.lane");
        let rep_idx = ctx.new_block("l2s.rep");
        let lane_label = ctx.block_label(lane_idx);
        let rep_label = ctx.block_label(rep_idx);
        ctx.block().cond_br(&holds, &lane_label, &miss_label);

        ctx.current_block = lane_idx;
        let blk = ctx.block();
        let stored_bits = blk.bitcast_double_to_i64(value_double);
        let plain = crate::expr::class_field_inline_guard::emit_plain_finite_number_check(
            blk,
            &stored_bits,
        );
        blk.cond_br(&plain, &meta_label, &rep_label);

        ctx.current_block = rep_idx;
        let blk = ctx.block();
        let rep_ptr = blk.gep(
            I8,
            &rec,
            &[(I64, &crate::runtime_abi::SHAPE_RECORD_REP.to_string())],
        );
        let rep = blk.load(I64, &rep_ptr);
        let lane = blk.lshr(I64, &rep, &(2 * slot).to_string());
        let lane = blk.and(I64, &lane, "3");
        let any = blk.icmp_eq(I64, &lane, "0");
        blk.cond_br(&any, &meta_label, &miss_label);
    }

    // `ObjectHeader.meta` is the header's last word.
    ctx.current_block = meta_idx;
    let blk = ctx.block();
    let meta_ptr =
        crate::expr::receiver_range::emit_field_ptr(blk, &recv.biased, (header - 8) as i64);
    let meta = blk.load(I64, &meta_ptr);
    let plain = blk.icmp_eq(I64, &meta, "0");
    blk.cond_br(&plain, &load_label, &miss_label);

    ctx.current_block = load_idx;
    let blk = ctx.block();
    let slot_ptr = crate::expr::receiver_range::emit_field_ptr(
        blk,
        &recv.biased,
        (header + 8 * slot as u64) as i64,
    );
    let current = blk.load(I64, &slot_ptr);
    let hole = blk.icmp_eq(I64, &current, TAG_HOLE_I64);
    blk.cond_br(&hole, &miss_label, &hit_label);

    ctx.current_block = hit_idx;
    // GC_STORE_AUDIT(BARRIERED): the bookkeeping below classifies the stored
    // bits exactly as the store site's hit does.
    ctx.block().store(DOUBLE, value_double, &slot_ptr);
    if !value_is_scalar {
        let handle = crate::expr::receiver_range::emit_handle(ctx.block(), &recv.biased);
        crate::expr::put_value_store_ic::emit_static_store_ic_bookkeeping(
            ctx,
            &handle,
            &slot_ptr,
            value_double,
            value_bits,
            "l2s",
        );
    }
    let hit_end = ctx.block_label(ctx.current_block);
    ctx.block().br(&merge_label);
    ctx.current_block = miss_idx;
    Some(LayoutStore {
        hit_label: hit_end,
        merge_idx,
    })
}

/// Are compiler-owned family stores emitted for this target?
fn family_stores_enabled(ctx: &FnCtx<'_>) -> bool {
    !(crate::target_layout::target_is_ilp32(ctx.target_triple)
        || ctx.target_triple.contains("wasm")
        || !crate::layout_desc::layouts_enabled("executable", ctx.target_triple))
}

/// One store of a family constructor's run (object model step 2c): `ok` (an
/// `i1`) is the run's check (`perry_family_run_admits`, which proved the
/// receiver a family object of the constructor's class or a descendant),
/// `slot` the slot every family descending from the class gives the key.
pub(crate) struct RunArm {
    pub ok: String,
    pub slot: u32,
    /// The stored key (its `PERRY_PROBE_VERIFY` slot witness).
    pub key: String,
}

/// A constructor run in flight in the innermost constructor body
/// (`InlineCtorReturn::family_run`): its check `ok` (an `i1`) and the key
/// position its last plain store wrote.
#[derive(Clone)]
pub(crate) struct FamilyRun {
    pub ok: String,
    pub last_pos: u32,
}

/// What evaluating a constructor store's value may do, given the run's check
/// holds (`perry_family_run_admits`: no prototype holds an accessor, a
/// non-writable or a private property named like a family key).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunValue {
    /// Runs no user code: literals, locals, `this`, `this.k` for a key `k`
    /// of the family (a plain data read on a checked chain), function and
    /// plain object and array literals of such values, an empty `new Map()`
    /// or `new Set()`, and `&&` `||` `??` `?:` `!` `===` `!==` `typeof`
    /// `void` of such values.
    Free,
    /// `this.k.bind(<Free>...)`: no user code when the call takes the
    /// builtin (`function_bind::emit_bind_call`, which arms the watch).
    Bind,
    /// May run user code: the run ends at it.
    Runs,
}

fn run_value(e: &Expr, keys: &[&str]) -> RunValue {
    fn free(e: &Expr, keys: &[&str]) -> bool {
        match e {
            Expr::Undefined
            | Expr::Null
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Integer(_)
            | Expr::String(_)
            | Expr::LocalGet(_)
            | Expr::This
            | Expr::Closure { .. } => true,
            Expr::PropertyGet {
                object, property, ..
            } => matches!(object.as_ref(), Expr::This) && keys.contains(&property.as_str()),
            Expr::Object(fields) => fields
                .iter()
                .all(|(k, v)| k != "__proto__" && free(v, keys)),
            Expr::Array(items, _) => items.iter().all(|v| free(v, keys)),
            // An empty collection: a direct runtime allocation, with no
            // constructor lookup and no iterable to drain.
            Expr::MapNew | Expr::SetNew => true,
            // ToBoolean, the nullish test, strict equality, `typeof` and
            // `void` convert nothing, so they call nothing.
            Expr::Logical { left, right, .. } => free(left, keys) && free(right, keys),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => free(condition, keys) && free(then_expr, keys) && free(else_expr, keys),
            Expr::Unary {
                op: perry_hir::UnaryOp::Not,
                operand,
            }
            | Expr::TypeOf(operand)
            | Expr::Void(operand) => free(operand, keys),
            Expr::Compare {
                op: perry_hir::CompareOp::Eq | perry_hir::CompareOp::Ne,
                left,
                right,
            } => free(left, keys) && free(right, keys),
            _ => false,
        }
    }
    if free(e, keys) {
        return RunValue::Free;
    }
    if let Expr::Call { callee, args, .. } = e {
        if let Expr::PropertyGet {
            object, property, ..
        } = callee.as_ref()
        {
            if property == "bind"
                && args.len() <= 3
                && matches!(object.as_ref(), Expr::PropertyGet { object: o, property: k, .. }
                    if matches!(o.as_ref(), Expr::This) && keys.contains(&k.as_str()))
                && args.iter().all(|a| free(a, keys))
            {
                return RunValue::Bind;
            }
        }
    }
    RunValue::Runs
}

/// A constructor store's place in its body's run of family stores.
pub(crate) struct RunPlan {
    pub pos: u32,
    /// No code since the birth can have run before this store (the chain's
    /// inert prologue, `FamilyLayout::init_count`): every later key is a hole.
    pub order_known: bool,
    /// The constructor's class.
    pub class_id: u32,
    pub slot: u32,
    pub value: RunValue,
    /// The run this store continues, taken from the body.
    pub run_in: Option<FamilyRun>,
}

/// The run plan of `this.<property> = value` in the innermost constructor
/// body being lowered, when `property` is a constructor key of the body's
/// class family stored after its unchecked initialization
/// (`FamilyLayout::init_count`) at a position and slot every family
/// descending from the class agrees on. Takes the body's run; a bind value
/// arms the body's bind watch (`function_bind::emit_bind_call`).
pub(crate) fn family_run_plan(
    ctx: &mut FnCtx<'_>,
    property: &str,
    value: &Expr,
) -> Option<RunPlan> {
    if ctx.inline_ctor_return.is_empty() {
        return None;
    }
    let mut plan = run_target(ctx, property, value)?;
    let body = ctx.inline_ctor_return.last_mut()?;
    plan.run_in = body.family_run.take();
    body.bind_watch = (plan.value == RunValue::Bind && plan.run_in.is_some()).then_some(None);
    Some(plan)
}

/// [`family_run_plan`] for a field definition (`lower_call::field_init`).
/// A base class's fields are defined at its constructor's entry, before the
/// body exists: their run is the phase's own, kept in `local`, and ends with
/// the phase. Inside a body (a derived class's fields, at its `super()`) it
/// is the body's run.
pub(crate) fn field_run_plan(
    ctx: &mut FnCtx<'_>,
    property: &str,
    value: &Expr,
    local: &mut Option<FamilyRun>,
) -> Option<RunPlan> {
    if !ctx.inline_ctor_return.is_empty() {
        return family_run_plan(ctx, property, value);
    }
    let mut plan = run_target(ctx, property, value)?;
    plan.run_in = local.take();
    Some(plan)
}

/// The key position, slot and value class of a run store of `property`
/// (see [`family_run_plan`]), with no run taken yet.
fn run_target(ctx: &mut FnCtx<'_>, property: &str, value: &Expr) -> Option<RunPlan> {
    if !family_stores_enabled(ctx) {
        return None;
    }
    let class_name = ctx.class_stack.last()?.clone();
    let class_id = *ctx.class_ids.get(&class_name)?;
    let fams = &ctx.program_layouts.families;
    let own = fams.get(&class_id)?;
    let pos = own.keys.iter().position(|(k, _)| k == property)?;
    if pos >= own.own_count as usize {
        return None;
    }
    let order_known = pos < own.init_count as usize;
    let keys: Vec<&str> = own.keys[..own.own_count as usize]
        .iter()
        .filter_map(|(k, _)| k.name())
        .collect();
    let value = run_value(value, &keys);
    let descends = |mut c: u32| {
        for _ in 0..64 {
            if c == class_id {
                return true;
            }
            match fams.get(&c) {
                Some(f) if f.parent_class_id != 0 => c = f.parent_class_id,
                _ => return false,
            }
        }
        false
    };
    // Every family layout descending from the class holds the key at the
    // same position and slot: the run's check proves the receiver one of
    // them, so each store of the run needs no receiver test of its own.
    let slot = own.keys[pos].1;
    for f in fams.values() {
        if !descends(f.class_id) {
            continue;
        }
        if f.keys.get(pos).map(|(k, s)| (k.name(), *s)) != Some((Some(property), slot))
            || f.variants.iter().any(|v| {
                v.keys.get(pos).map(|(k, s)| (k.name(), *s)) != Some((Some(property), slot))
            })
        {
            crate::expr::note_site("case2c-run-disagree");
            return None;
        }
    }
    Some(RunPlan {
        pos: pos as u32,
        order_known,
        class_id,
        slot,
        value,
        run_in: None,
    })
}

/// The run arm of a planned store, emitted after its value: the run it
/// continues when the value runs no user code and the store writes a later
/// key position than the run's last plain store, else a fresh check. Returns
/// the arm (`None`: the store takes the site alone, the run continuing) and
/// records the run that follows the store in the body.
pub(crate) fn family_run_arm(
    ctx: &mut FnCtx<'_>,
    plan: RunPlan,
    this_bits: &str,
    property: &str,
) -> Option<RunArm> {
    let (out, run) = run_arm(ctx, plan, this_bits, property);
    if let Some(body) = ctx.inline_ctor_return.last_mut() {
        body.family_run = Some(run);
    }
    out
}

/// [`family_run_arm`] for a field definition planned by [`field_run_plan`]:
/// the run that follows it goes back where the plan took it from.
pub(crate) fn field_run_arm(
    ctx: &mut FnCtx<'_>,
    plan: RunPlan,
    this_bits: &str,
    property: &str,
    local: &mut Option<FamilyRun>,
) -> Option<RunArm> {
    if !ctx.inline_ctor_return.is_empty() {
        return family_run_arm(ctx, plan, this_bits, property);
    }
    let (out, run) = run_arm(ctx, plan, this_bits, property);
    *local = Some(run);
    out
}

fn run_arm(
    ctx: &mut FnCtx<'_>,
    plan: RunPlan,
    this_bits: &str,
    property: &str,
) -> (Option<RunArm>, FamilyRun) {
    let watched = ctx
        .inline_ctor_return
        .last_mut()
        .and_then(|b| b.bind_watch.take())
        .flatten();
    // Lowering visits sibling arms sequentially, but a run established in
    // one arm is not available in the other or beyond a bypassing join.
    // Check after the RHS too: it may introduce control flow of its own.
    let run_in = plan.run_in.filter(|r| {
        plan.value != RunValue::Runs && ctx.func.proof_dominates_block_end(&r.ok, ctx.current_block)
    });
    let carried = match (plan.value, &run_in) {
        (RunValue::Free, Some(r)) => Some(r.clone()),
        (RunValue::Bind, Some(r)) => watched.map(|b| FamilyRun {
            ok: ctx.block().and(crate::types::I1, &r.ok, &b),
            last_pos: r.last_pos,
        }),
        _ => None,
    };
    let (run, arm) = match carried {
        // A key the run already passed: the site stores it (an overwrite,
        // or an out-of-order add the store probe orders), the run goes on.
        Some(r) if plan.pos <= r.last_pos => (r, false),
        Some(r) => (
            FamilyRun {
                ok: r.ok,
                last_pos: plan.pos,
            },
            true,
        ),
        None => {
            crate::expr::note_site("case2c-run-check");
            let (dir, _key_bits, hash) = crate::expr::method_site::emit_site_key(ctx, property);
            // PLAN 3c: the site proves the run from the receiver's class and
            // its static chain (`proto_constants::emit_family_run_proof`) and
            // asks the runtime only when that proof refuses.
            let proof = crate::proto_constants::emit_family_run_proof(
                ctx,
                this_bits,
                &dir,
                property,
                plan.pos,
                plan.order_known,
                plan.class_id,
            );
            if let Some((_, refused)) = proof {
                ctx.current_block = refused;
            }
            // Only an object at a family layout can be admitted: a static
            // layout (a family's), or an unshadowed linked one (a class
            // evaluation's copy of a family's, `layout_desc::family_desc_of`).
            // Any other receiver (an instance of a subclass without a family
            // running its parent's constructor) is refused here, unasked.
            let band = ctx.new_block("crun.band");
            let ask = ctx.new_block("crun.ask");
            let asked_join = ctx.new_block("crun.asked");
            let band_l = ctx.block_label(band);
            let ask_l = ctx.block_label(ask);
            let asked_l = ctx.block_label(asked_join);
            let recv =
                crate::expr::receiver_range::emit_fused_receiver_test(ctx.block(), this_bits);
            let from_entry = ctx.block().label.clone();
            ctx.block()
                .cond_br(&recv.is_object_pointer, &band_l, &asked_l);
            ctx.current_block = band;
            let blk = ctx.block();
            let word_ptr = crate::expr::receiver_range::emit_field_ptr(blk, &recv.biased, 4);
            let word = blk.load(crate::types::I32, &word_ptr);
            let in_band = emit_family_word_test(blk, &word);
            let from_band = blk.label.clone();
            blk.cond_br(&in_band, &ask_l, &asked_l);
            ctx.current_block = ask;
            let c = ctx.block().call(
                crate::types::I32,
                "perry_family_run_admits",
                &[
                    (crate::types::PTR, &dir),
                    (I64, this_bits),
                    (I64, &hash),
                    (crate::types::I32, &plan.pos.to_string()),
                    (crate::types::I32, if plan.order_known { "1" } else { "0" }),
                    (crate::types::I32, &plan.class_id.to_string()),
                ],
            );
            let from_ask = ctx.block().label.clone();
            ctx.block().br(&asked_l);
            ctx.current_block = asked_join;
            let c = ctx.block().phi(
                crate::types::I32,
                &[("0", &from_entry), ("0", &from_band), (&c, &from_ask)],
            );
            let asked = ctx.block().icmp_ne(crate::types::I32, &c, "0");
            let ok = match proof {
                None => asked,
                Some((proved, _)) => {
                    let join = ctx.new_block("crun.join");
                    let join_l = ctx.block_label(join);
                    let asked_end = ctx.block().label.clone();
                    ctx.block().br(&join_l);
                    ctx.current_block = proved;
                    let proved_end = ctx.block().label.clone();
                    ctx.block().br(&join_l);
                    ctx.current_block = join;
                    ctx.block().phi(
                        crate::types::I1,
                        &[("true", &proved_end), (&asked, &asked_end)],
                    )
                }
            };
            (
                FamilyRun {
                    ok,
                    last_pos: plan.pos,
                },
                true,
            )
        }
    };
    let out = arm.then(|| RunArm {
        ok: run.ok.clone(),
        slot: plan.slot,
        key: property.to_owned(),
    });
    (out, run)
}

/// Admit a static PLAIN family word in the image's contiguous family range,
/// or an ordinary MIXED evaluation word. The runtime recovers the immutable
/// family descriptor from the live record and verifies the requested family.
fn emit_family_word_test(blk: &mut crate::block::LlBlock, word: &str) -> String {
    let plan = crate::layout_ordinals::current_plan();
    let base = crate::runtime_abi::LAYOUT_TAG | plan.family_first;
    let rel = blk.sub(I32, word, &(base as i32).to_string());
    let in_static = blk.icmp_ult(I32, &rel, &plan.family_count.to_string());
    let kind = blk.and(
        I32,
        word,
        &(crate::runtime_abi::LAYOUT_KIND_MASK as i32).to_string(),
    );
    let linked = blk.icmp_eq(
        I32,
        &kind,
        &(crate::runtime_abi::LAYOUT_KIND_MASK as i32).to_string(),
    );
    let ordinary_rel = blk.sub(
        I32,
        word,
        &(crate::runtime_abi::LAYOUT_TAG as i32).to_string(),
    );
    let unshadowed = blk.icmp_ult(
        I32,
        &ordinary_rel,
        &((crate::runtime_abi::LAYOUT_ORDINARY_END - crate::runtime_abi::LAYOUT_TAG) as i32)
            .to_string(),
    );
    let copy = blk.and(crate::types::I1, &linked, &unshadowed);
    blk.or(crate::types::I1, &in_static, &copy)
}

/// Object model step 2c (A): a constructor key's store through `this` in a
/// run of the constructor's family stores ([`family_run_arm`]) is a plain
/// slot store when the run's check holds, the receiver is at one of the
/// family layouts of the constructor's class or of a family descendant
/// (their class ids, and a static layout word without the shadow bit: no
/// overflow orders it). The check proved the store plain (nothing on the
/// chain intercepts a family key) and ordered (from the run's first key on,
/// no key present after a hole), and the run's stores go in key order with no
/// user code between them: a present key's store overwrites its own data, a
/// hole's adds the key with every later key still a hole.
/// Every other case is the site below. The returned arm is joined by
/// [`finish_store`].
pub(crate) fn begin_family_init_store(
    ctx: &mut FnCtx<'_>,
    receiver_bits: &str,
    value_double: &str,
    value_bits: &str,
    value_is_scalar: bool,
    run: Option<&RunArm>,
) -> Option<LayoutStore> {
    let arm = run?;
    debug_assert!(
        ctx.func
            .proof_dominates_block_end(&arm.ok, ctx.current_block),
        "family-run proof {} must dominate its store gate",
        arm.ok,
    );
    let slot = arm.slot;
    crate::expr::note_site("case2c-run");
    let header =
        crate::target_layout::object_slot_base_bytes(ctx.target_triple, ctx.program_layouts.form());
    let gate_idx = ctx.new_block("l2i.gate");
    let word_idx = ctx.new_block("l2i.word");
    let load_idx = ctx.new_block("l2i.load");
    let hit_idx = ctx.new_block("l2i.hit");
    let miss_idx = ctx.new_block("l2i.miss");
    let merge_idx = ctx.new_block("l2i.merge");
    let word_label = ctx.block_label(word_idx);
    let load_label = ctx.block_label(load_idx);
    let hit_label = ctx.block_label(hit_idx);
    let miss_label = ctx.block_label(miss_idx);

    // A run's store: its run's one check holds, else the site below.
    let gate_label = ctx.block_label(gate_idx);
    ctx.block().cond_br(&arm.ok, &gate_label, &miss_label);
    ctx.current_block = gate_idx;
    let recv = crate::expr::receiver_range::emit_fused_receiver_test(ctx.block(), receiver_bits);
    ctx.block()
        .cond_br(&recv.is_object_pointer, &word_label, &miss_label);

    ctx.current_block = word_idx;
    // The run's check proved the receiver a family object of the class (or
    // a descendant), whose layouts all hold the key at `slot`; its word still
    // carries no shadow bit (a store of the run that took the site below may
    // have given it an overflow, which orders its keys from then on).
    let blk = ctx.block();
    let word_ptr = crate::expr::receiver_range::emit_field_ptr(blk, &recv.biased, 4);
    let word = blk.load(I32, &word_ptr);
    let in_band = emit_family_word_test(blk, &word);
    blk.cond_br(&in_band, &load_label, &miss_label);

    ctx.current_block = load_idx;
    super::class_family_read::emit_slot_verify(ctx, receiver_bits, &arm.key, slot);
    let blk = ctx.block();
    let slot_ptr = crate::expr::receiver_range::emit_field_ptr(
        blk,
        &recv.biased,
        (header + 8 * slot as u64) as i64,
    );
    blk.br(&hit_label);

    ctx.current_block = hit_idx;
    // GC_STORE_AUDIT(BARRIERED): the bookkeeping below classifies the stored
    // bits exactly as the store site's hit does.
    ctx.block().store(DOUBLE, value_double, &slot_ptr);
    if !value_is_scalar {
        let handle = crate::expr::receiver_range::emit_handle(ctx.block(), &recv.biased);
        crate::expr::put_value_store_ic::emit_static_store_ic_bookkeeping(
            ctx,
            &handle,
            &slot_ptr,
            value_double,
            value_bits,
            "l2i",
        );
    }
    let hit_end = ctx.block_label(ctx.current_block);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().br(&merge_label);
    ctx.current_block = miss_idx;
    Some(LayoutStore {
        hit_label: hit_end,
        merge_idx,
    })
}

/// Join the 2c store hit (whose value is `value_double`) with the ordinary
/// site's result `generic` (produced in the current block).
pub(crate) fn finish_store(
    ctx: &mut FnCtx<'_>,
    arm: LayoutStore,
    value_double: &str,
    generic: &str,
) -> String {
    let merge_label = ctx.block_label(arm.merge_idx);
    let generic_label = ctx.block_label(ctx.current_block);
    ctx.block().br(&merge_label);
    ctx.current_block = arm.merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (value_double, arm.hit_label.as_str()),
            (generic, generic_label.as_str()),
        ],
    )
}
