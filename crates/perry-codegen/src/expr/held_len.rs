//! A loop that holds `a.length` across its calls (wpfacts
//! `BindingFacts::stable_length`, `perry_wpfacts` `held_lengths`;
//! CENSUS-residual-2 lever 8).
//!
//! `for (const x of f())` walks `for (let k = 0; k < t.length; k++) { const
//! x = t[k]; body }` (`perry_hir` `for_of_guard`), and a body that calls
//! anything re-reads `t.length` and re-derives the element slot at every
//! step: the loop's own walker (`stmt::loops::classify_for_length_hoist`)
//! stops at the first call, since a call may push onto `t`. When the facts
//! say no code that runs during an activation of this function changes the
//! length of an Array `t` may hold (the Array lives in frames only, this
//! function hands it to no call and has no length event on it), the loop is
//! the hoisted loop all the same: the length is read once into a slot, the
//! counter is compared with it as an `i32`, and `t[k]` is in bounds.
//!
//! **What is held, and what is not.** Only the length, a scalar: it is the
//! same number wherever a collection moves the Array. The Array's address
//! is read from its root slot at every read, as before, so a moving
//! collection inside a callee needs no re-derivation here.
//!
//! **The read** (`lower_read`) is the rep-keyed tier's
//! (`index_get::proven_array`) with `RepRead::bounded`: no `idx <u
//! capacity` test, and, when every Array the local may hold has a defined
//! length (`stable_front`), the slot at `h + 24 + 8 * idx` with no capacity
//! or size load. The forwarding test stays (the value in the local may be
//! an Array's old header from before the loop), as do the hole test and, in
//! a number context, the plain-Number test.
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`): every read of the held length
//! reads the live length too and reports a difference ("held length", exit
//! 86); a `stable_front` read compares its address with the one computed
//! through the header's capacity and size. `PERRY_FACTS_HELD_LENGTHS=0`
//! turns the fact and this loop off (the compile-cache key holds it).
//! `PERRY_ARRAY_BASE_DIAG=1` prints one `[perry held len]` line per loop.

use anyhow::Result;
use perry_hir::Expr;

use super::index_get::proven_array;
use super::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I32, I64, I8};

/// One loop that holds the length of `arr` by the facts, with `counter`
/// its bounded index (`FnCtx::held_lens`): the loop tests `counter <
/// arr.length` before every run of its body, nothing in it assigns either,
/// and `counter` starts non-negative (`stmt::held_len::classify`), so
/// `counter` is in `[0, length)` wherever the body reads it.
#[derive(Clone, Copy, Debug)]
pub struct HeldLen {
    pub(crate) arr: u32,
    pub(crate) counter: u32,
    /// `stable_front`: the elements start right behind the header.
    pub(crate) front: bool,
}

/// `PERRY_FACTS_HELD_LENGTHS=0` turns the held length off.
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_FACTS_HELD_LENGTHS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// Local `id` has a stable length by the facts: `Some(front)`.
pub(crate) fn stable(ctx: &FnCtx<'_>, id: u32) -> Option<bool> {
    if !enabled()
        || ctx.wpfacts_off
        || ctx.wpfacts.interp_guard
        || super::typed_feedback_emission_enabled()
    {
        return None;
    }
    ctx.wpfacts.stable_length_locals.get(&id).copied()
}

pub(crate) fn diag(ctx: &FnCtx<'_>, arr: u32, verdict: &str) {
    if std::env::var("PERRY_ARRAY_BASE_DIAG").as_deref() == Ok("1") {
        eprintln!(
            "[perry held len] fn={} array={arr} verdict={verdict}",
            ctx.func.name
        );
    }
}

/// The loop that holds `arr`'s length by the facts, innermost first.
fn held(ctx: &FnCtx<'_>, arr: u32) -> Option<HeldLen> {
    ctx.held_lens.iter().rev().find(|h| h.arr == arr).copied()
}

/// Check mode: `ok` (an `i1`) must hold, else `value` is reported as a
/// false "held length" proof.
fn witness(ctx: &mut FnCtx<'_>, ok: &str, value: &str, what: &str) {
    let miss = ctx.new_block("heldlen.verify_miss");
    let next = ctx.new_block("heldlen.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    crate::verify_facts::tick(ctx);
    ctx.block().cond_br(ok, &next_l, &miss_l);
    ctx.current_block = miss;
    let site = crate::verify_facts::site_arg(ctx, what, crate::verify_facts::Fact::HeldLength, &[]);
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

/// A read of the cached length `cached` of local `arr` (`expr::
/// property_get`): the consumption record, and in check mode the test that
/// the live length is still that number.
pub(crate) fn length_read(ctx: &mut FnCtx<'_>, arr: u32, cached: &str) -> Result<()> {
    if held(ctx, arr).is_none() {
        return Ok(());
    }
    crate::facts_consumed::used();
    crate::verify_facts::covered(
        crate::verify_facts::Fact::HeldLength,
        || serde_json::json!({ "local": arr, "use": "length" }),
    );
    if !crate::verify_facts::enabled() {
        return Ok(());
    }
    let arr_box = lower_expr(ctx, &Expr::LocalGet(arr))?;
    let live = ctx.block().call(
        DOUBLE,
        "js_value_length_property_f64",
        &[(DOUBLE, &arr_box)],
    );
    let same = ctx.block().fcmp("oeq", &live, cached);
    witness(
        ctx,
        &same,
        &arr_box,
        &format!("local {arr} keeps its length"),
    );
    Ok(())
}

/// `object[index]` where `object` is a local whose length the loop holds
/// and `index` its bounded counter: the bounded rep-keyed read (module
/// docs). `None` for any other read.
pub(crate) fn lower_read(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    element_offset: u32,
    numeric: bool,
) -> Result<Option<String>> {
    let (Expr::LocalGet(arr), Expr::LocalGet(idx)) = (object, index) else {
        return Ok(None);
    };
    let Some(h) = held(ctx, *arr).filter(|h| h.counter == *idx) else {
        return Ok(None);
    };
    let Some(i32_slot) = ctx.i32_counter_slots.get(idx).cloned() else {
        return Ok(None);
    };
    let Some(mut read) = proven_array::rep_read(ctx, object, element_offset) else {
        return Ok(None);
    };
    // An at-rest Array's read is its own tier's.
    if read.rep.at_rest {
        return Ok(None);
    }
    read.bounded = Some(h.front);
    crate::verify_facts::covered(
        crate::verify_facts::Fact::HeldLength,
        || serde_json::json!({ "local": arr, "use": "element", "front": h.front }),
    );
    let arr_box = lower_expr(ctx, object)?;
    let idx_i32 = ctx.block().load(I32, &i32_slot);
    if crate::verify_facts::enabled() {
        let live = ctx.block().call(
            DOUBLE,
            "js_value_length_property_f64",
            &[(DOUBLE, &arr_box)],
        );
        let idx_f = ctx.block().sitofp(I32, &idx_i32, DOUBLE);
        let inside = ctx.block().fcmp("olt", &idx_f, &live);
        witness(
            ctx,
            &inside,
            &arr_box,
            &format!("local {arr} holds an element at its bounded index"),
        );
    }
    Ok(Some(proven_array::lower_held(
        ctx, &arr_box, &idx_i32, read, numeric,
    )))
}

/// Check mode, on a live (not forwarded) header `handle`: the front slot
/// address `front` of `idx_i32` is the one the header's capacity and size
/// give.
pub(crate) fn witness_front(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    handle: &str,
    idx_i32: &str,
    front: &str,
) {
    if !crate::verify_facts::enabled() {
        return;
    }
    let blk = ctx.block();
    let ptr = blk.inttoptr(I64, handle);
    let cap_ptr = blk.gep(I8, &ptr, &[(I64, "4")]);
    let cap = blk.load(I32, &cap_ptr);
    let generic = blk.array_element_addr_with_capacity(handle, &cap, idx_i32);
    let same = blk.icmp_eq(I64, &generic, front);
    witness(ctx, &same, arr_box, "the elements start behind the header");
}
