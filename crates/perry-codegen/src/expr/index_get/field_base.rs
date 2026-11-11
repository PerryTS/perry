//! The element base of an Array held in a field of `this`, held across a
//! loop (DESIGN-p4l1 rev 2 §4.3, PR-D; REVIEW-p4l1 findings 10 and 11).
//!
//! `stmt::field_base` decides which loop holds which field's base. This
//! module emits what the base needs: the install, the refresh, and the
//! reads of the field read calls (b1, `lower_call::field_read_call`) the
//! loop serves from it.
//!
//! **Install.** Load `this.f` (the field's `FieldProof.array` is `Arr`,
//! dense, with a floor), mask it to its handle, and resolve forwarding
//! once: a header with `GC_FLAG_FORWARDED` is a growth stub, and the
//! runtime's `js_array_live_head` follows the chain to the live head (one
//! cold call, no inline edge). The facts give the kind; forwarding is only
//! an address, so there is no second loop body (finding 10). The live head
//! is stored in the base slot. With `fixed` storage nothing is ever
//! forwarded and the head is the handle.
//!
//! **Refresh.** The base is an address: the back-edge poll's slow block
//! (`stmt::loops`) re-derives it from `this`'s GC-updated root, exactly as
//! at install, and so does the cold arm of a read that may collect.
//!
//! **Reads.** A call in its own floor (wpfacts `FieldReadCall::in_floor`)
//! is `load [base + 24 + 8 * i]`. Any other call is `i <u floor ? load
//! [base + 24 + 8 * i] : <the call's read>`: below the dense floor the slot
//! is a present element, and outside it the cold arm is the read the call
//! has without a base (C3's), after which every held base is re-derived,
//! since that read's generic `Get` may collect. With a `?? L`, the elements
//! must all be Numbers (a stored `null` or `undefined` is a present
//! element), as for C3's in-floor call.
//!
//! **Check mode** (`PERRY_VERIFY_FACTS=1`). At install and at every
//! refresh, the head is tested as a field read's `Arr` claim is, and it
//! must not be forwarded. At every read, the base held must be the live
//! head of a fresh load of the field, and C3's read witness runs on that
//! fresh load. b1's own witness (the real call beside the read) may
//! collect, so the bases are re-derived after it. A miss exits 86
//! (`Fact::ArrayRep`).
//!
//! `PERRY_FACTS_ARRAY_BASE=0` turns the installer off (C3's build).
//! Negative controls: `PERRY_FACTS_SABOTAGE=arrbase` installs the stub's
//! handle without resolving forwarding; `restheld` reads an at-rest Array's
//! I32 word as a double (I32 at rest B2); `arrbaseadm` admits a loop whatever
//! else ends its region (`stmt::field_base`); `arrbasepoll` skips the
//! poll's refresh (exposed only by a moving collection at the poll:
//! `PERRY_GC_SCHEDULE_SEED` with the instrumented runtime).

use anyhow::Result;
use perry_hir::{Expr, LogicalOp};

use super::FnCtx;
use crate::codegen::{FactsArrRep, FactsElemRep};
use crate::collectors::FieldBase;
use crate::nanbox::{POINTER_MASK_I64, POINTER_TAG_I64};
use crate::types::{DOUBLE, I32, I64};

/// `perry_abi::GC_FLAG_FORWARDED` in the header word's second byte.
const FORWARDED_I32: &str = "32768";

/// `PERRY_FACTS_ARRAY_BASE=0` turns the installer off.
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_FACTS_ARRAY_BASE").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// The negative control `PERRY_FACTS_SABOTAGE=<arm>`.
pub(crate) fn sabotaged(arm: &str) -> bool {
    std::env::var("PERRY_FACTS_SABOTAGE").as_deref() == Ok(arm)
}

/// A field read call's read (`this.f[k] ?? L`, `field_read_call_at`) that a
/// held base can serve: the field, its representation and the field read
/// `this.f` itself.
pub(crate) struct Servable<'e> {
    pub(crate) field: &'e str,
    pub(crate) rep: FactsArrRep,
    pub(crate) field_read: &'e Expr,
    pub(crate) index: &'e Expr,
}

/// `read` (a field read call's rebuilt read), when a base of `this.f` can
/// serve it: the receiver is `this`, the field read is `Arr`, dense with a
/// floor (`proven_array::in_floor_rep`), and a `?? L` is over Number
/// elements.
pub(crate) fn servable<'e>(ctx: &FnCtx<'_>, read: &'e Expr) -> Option<Servable<'e>> {
    let (element, coalesced) = match read {
        Expr::Logical {
            op: LogicalOp::Coalesce,
            left,
            ..
        } => (left.as_ref(), true),
        e => (e, false),
    };
    let Expr::IndexGet { object, index, .. } = element else {
        return None;
    };
    let Expr::PropertyGet {
        object: holder,
        property,
        ..
    } = object.as_ref()
    else {
        return None;
    };
    if !matches!(holder.as_ref(), Expr::This) {
        return None;
    }
    let rep = super::proven_array::in_floor_rep(ctx, object)?;
    if coalesced && rep.elem == FactsElemRep::Boxed {
        return None;
    }
    Some(Servable {
        field: property,
        rep,
        field_read: object,
        index,
    })
}

/// Load the field, resolve forwarding and store the live head in the base
/// slot (module docs): the install and every refresh.
pub(crate) fn emit_install(ctx: &mut FnCtx<'_>, base: &FieldBase) -> Result<()> {
    let boxed = super::lower_expr(ctx, &base.read)?;
    let bits = ctx.block().bitcast_double_to_i64(&boxed);
    let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    let head = if base.rep.unmoved() || sabotaged("arrbase") {
        handle
    } else {
        live_head(ctx, &handle)
    };
    if crate::verify_facts::enabled() {
        let tagged = ctx.block().or(I64, &head, POINTER_TAG_I64);
        let word = ctx.block().bitcast_i64_to_double(&tagged);
        // The head is never a stub: tested as fixed storage is.
        let r = FactsArrRep {
            fixed: true,
            ..base.rep
        };
        super::array_rep::witness_value(ctx, &word, r, &format!("field base this.{}", base.field));
    }
    ctx.block().store(I64, &head, &base.base_slot);
    Ok(())
}

/// The live head of `handle`: itself, or the end of its forwarding chain.
fn live_head(ctx: &mut FnCtx<'_>, handle: &str) -> String {
    let word = super::guarded_array::emit_array_guard_word(ctx.block(), handle);
    let moved = ctx.block().and(I32, &word, FORWARDED_I32);
    let stub = ctx.block().icmp_ne(I32, &moved, "0");
    let follow = ctx.new_block("abase.follow");
    let live = ctx.new_block("abase.live");
    let (follow_l, live_l) = (ctx.block_label(follow), ctx.block_label(live));
    let from = ctx.block().label.clone();
    ctx.block().cond_br(&stub, &follow_l, &live_l);
    ctx.current_block = follow;
    let followed = ctx
        .block()
        .call(I64, "js_array_live_head", &[(I64, handle)]);
    ctx.block().br(&live_l);
    ctx.current_block = live;
    ctx.block()
        .phi(I64, &[(handle, &from), (&followed, &follow_l)])
}

/// Re-derive every held base: after a call that may collect.
pub(crate) fn rederive_all(ctx: &mut FnCtx<'_>) -> Result<()> {
    for base in ctx.receiver_descriptors.field_bases() {
        emit_install(ctx, &base)?;
    }
    Ok(())
}

/// The back-edge poll's slow block, after `js_gc_loop_safepoint`: every
/// held base, from `this`'s GC-updated root.
pub(crate) fn emit_poll_refresh(ctx: &mut FnCtx<'_>) -> Result<()> {
    let bases = ctx
        .receiver_descriptors
        .field_base_refreshes()
        .expect("every held field base must admit a back-edge poll");
    if sabotaged("arrbasepoll") {
        return Ok(());
    }
    for base in &bases {
        emit_install(ctx, base)?;
    }
    Ok(())
}

/// The read of the field read call at `site` (rebuilt as `read`), from the
/// base an enclosing loop holds; `None` when no loop holds that field's
/// base or the read is not one a base serves.
pub(crate) fn try_lower_read(
    ctx: &mut FnCtx<'_>,
    site: u32,
    read: &Expr,
) -> Result<Option<String>> {
    let Some(s) = servable(ctx, read) else {
        return Ok(None);
    };
    let Some(base) = ctx.receiver_descriptors.field_base(s.field).cloned() else {
        return Ok(None);
    };
    let call_floor = ctx
        .wpfacts
        .field_read_calls
        .get(&site)
        .and_then(|f| f.in_floor);
    let index = s.index;
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "field_base": base.field, "floor": base.rep.floor, "in_floor": call_floor.is_some() }),
    );
    crate::facts_consumed::used();
    let idx = crate::expr::lower_expr_as_i32(ctx, index)?;
    if crate::verify_facts::enabled() {
        witness(ctx, &base, &idx, call_floor)?;
    }
    if call_floor.is_some() {
        return Ok(Some(load(ctx, &base, &idx)));
    }
    // Outside the call's floor: the slot below the dense floor, else the
    // call's own read, after which the bases are re-derived.
    let fast = ctx.new_block("abase.slot");
    let slow = ctx.new_block("abase.read");
    let merge = ctx.new_block("abase.merge");
    let (fast_l, slow_l, merge_l) = (
        ctx.block_label(fast),
        ctx.block_label(slow),
        ctx.block_label(merge),
    );
    let inside = ctx.block().icmp_ult(I32, &idx, &base.rep.floor.to_string());
    ctx.block().cond_br(&inside, &fast_l, &slow_l);
    ctx.current_block = fast;
    let value = load(ctx, &base, &idx);
    let fast_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = slow;
    let cold = super::lower_expr(ctx, read)?;
    rederive_all(ctx)?;
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = merge;
    Ok(Some(
        ctx.block()
            .phi(DOUBLE, &[(&value, &fast_end), (&cold, &slow_end)]),
    ))
}

/// `load [base + 24 + 8 * idx]`; an at-rest Array's I32 word is decoded
/// (`rest_elems::decode`, B2).
fn load(ctx: &mut FnCtx<'_>, base: &FieldBase, idx: &str) -> String {
    let blk = ctx.block();
    let head = blk.load(I64, &base.base_slot);
    let wide = blk.zext(I32, idx, I64);
    let offset = blk.shl(I64, &wide, "3");
    let elements = blk.add(
        I64,
        &head,
        &crate::target_layout::ARRAY_HEADER_SIZE_BYTES.to_string(),
    );
    let addr = blk.add(I64, &elements, &offset);
    let slot = blk.inttoptr(I64, &addr);
    let raw = blk.load(I64, &slot);
    // Negative control `restheld`: the word is taken as a double.
    if base.rep.at_rest && !sabotaged("restheld") {
        return crate::expr::rest_elems::decode(ctx, &raw, crate::expr::rest_elems::Want::Value);
    }
    blk.bitcast_i64_to_double(&raw)
}

/// Check mode at a read served from a held base (module docs).
fn witness(
    ctx: &mut FnCtx<'_>,
    base: &FieldBase,
    idx: &str,
    call_floor: Option<u32>,
) -> Result<()> {
    crate::verify_facts::covered(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "field_base": base.field, "floor": base.rep.floor }),
    );
    let fresh = super::lower_expr(ctx, &base.read)?;
    let bits = ctx.block().bitcast_double_to_i64(&fresh);
    let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    let live = ctx
        .block()
        .call(I64, "js_array_live_head", &[(I64, &handle)]);
    let held = ctx.block().load(I64, &base.base_slot);
    let same = ctx.block().icmp_eq(I64, &live, &held);
    let miss = ctx.new_block("abase.verify_miss");
    let next = ctx.new_block("abase.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    crate::verify_facts::tick(ctx);
    ctx.block().cond_br(&same, &next_l, &miss_l);
    ctx.current_block = miss;
    let site = crate::verify_facts::site_arg(
        ctx,
        &format!(
            "array rep of field base this.{}: the held base is not the live head",
            base.field
        ),
        crate::verify_facts::Fact::ArrayRep,
        &[],
    );
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, &fresh)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
    let read = super::proven_array::RepRead {
        rep: base.rep,
        in_floor: call_floor.is_some(),
        key_floor: call_floor.unwrap_or(base.rep.floor),
        below_length: false,
        number: base.rep.elem != FactsElemRep::Boxed,
        receiver: super::proven_array::Receiver::Field,
        bounded: None,
    };
    super::array_rep::witness_read(ctx, &fresh, idx, read, false);
    Ok(())
}
