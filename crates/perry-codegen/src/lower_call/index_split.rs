//! A call of an indexed method (`$idx_u31` clone) whose index arguments are
//! proven Numbers.
//!
//! The public entry of such a method guards a boxed argument with the exact
//! i32 test: a double that converts exactly, then a sign test. A call site
//! that proves the argument a canonical double
//! (`expr_produces_canonical_raw_f64`) applies that predicate
//! (`emit_typed_i32_plain_guard_and_raw`) inline and selects the clone or the generic body itself: the clone
//! admits exactly the values the public guard admits (no `-0`, NaN, fraction
//! or value outside the i32 range).
//!
//! `PERRY_NUMBER_INDEX_SPLIT=0` keeps the call on the public entry.

use perry_hir::Expr;

use crate::expr::FnCtx;
use crate::types::{LlvmType, DOUBLE, I1, I32};

fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_NUMBER_INDEX_SPLIT").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// One index argument of an indexed method call: its position, and the local
/// it reads when the whole-program interval proof shows that local a
/// non-negative int32 (`facts_int32_locals`): the clone's raw body then takes
/// it with no guard.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IndexArg {
    pub pos: usize,
    pub proven: Option<Proof>,
}

/// The local `arg` reads when it is an admitted int32 `let` whose interval is
/// non-negative.
/// How an index argument is known to be a non-negative int32.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Proof {
    /// An admitted int32 local with a non-negative interval
    /// (`facts_int32_locals`).
    Local(u32),
    /// The call site's interval for this argument (wpfacts
    /// `call_arg_ranges`, after narrowing); the local it reads, if any.
    Site(Option<u32>),
}

/// The wpfacts interval of argument `pos` at the call at `offset` lies in
/// `[0, i32::MAX]`.
fn site_nonneg(ctx: &FnCtx<'_>, offset: u32, pos: usize) -> bool {
    crate::codegen::index_entry::raw_i32_enabled()
        && site_args_enabled()
        && !ctx.wpfacts_off
        && offset != 0
        && ctx
            .wpfacts
            .call_arg_ranges
            .get(&offset)
            .and_then(|v| v.get(pos).copied().flatten())
            .is_some_and(|(lo, hi)| lo >= 0 && hi <= i64::from(i32::MAX))
}

thread_local! {
    /// The source offset of the method call being lowered (its callee's
    /// `PropertyGet::byte_offset`, the key of wpfacts `call_arg_ranges`).
    static SITE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Lower method call `callee` with `f`, its site offset visible to the
/// index split (`site_nonneg`); nested calls in its arguments set theirs.
pub(crate) fn with_call_site<R>(callee: &Expr, f: impl FnOnce() -> R) -> R {
    let offset = match callee {
        Expr::PropertyGet { byte_offset, .. } => *byte_offset,
        _ => 0,
    };
    let saved = SITE.with(|s| s.replace(offset));
    let out = f();
    SITE.with(|s| s.set(saved));
    out
}

/// `PERRY_FACTS_SITE_ARGS=0`: no call-site argument intervals.
fn site_args_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_FACTS_SITE_ARGS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

fn proven_nonneg_local(ctx: &FnCtx<'_>, arg: &Expr) -> Option<u32> {
    let Expr::LocalGet(id) = arg else {
        return None;
    };
    (crate::codegen::index_entry::raw_i32_enabled()
        && !ctx.wpfacts_off
        && ctx.facts_int32_locals.contains(id)
        && ctx.wpfacts.nonneg_int32_inputs.contains(id))
    .then_some(*id)
}

/// The argument positions of `class_name.property`'s index parameters when
/// the method has an `$idx_u31` clone and every such argument is a proven
/// canonical double or a proven non-negative int32 local, with the clone's
/// parameter-id list.
pub(crate) fn number_index_positions(
    ctx: &FnCtx<'_>,
    class_name: &str,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
) -> Option<(Vec<IndexArg>, Vec<u32>)> {
    if !enabled() {
        return None;
    }
    let params = ctx
        .nonnegative_index_methods
        .get(&(class_name.to_string(), property.to_string()))?;
    let method = ctx
        .classes
        .get(class_name)?
        .methods
        .iter()
        .find(|m| m.name == property)?;
    if args.len() != method.params.len() {
        return None;
    }
    let mut positions = Vec::with_capacity(params.len());
    for id in params {
        let pos = method.params.iter().position(|p| p.id == *id)?;
        let arg = args.get(pos)?;
        let site = SITE.with(std::cell::Cell::get);
        let _ = call_byte_offset;
        let proven = proven_nonneg_local(ctx, arg).map(Proof::Local).or_else(|| {
            site_nonneg(ctx, site, pos).then(|| match arg {
                Expr::LocalGet(id) => Proof::Site(Some(*id)),
                _ => Proof::Site(None),
            })
        });
        if proven.is_none() && !crate::type_analysis::expr_produces_canonical_raw_f64(ctx, arg) {
            return None;
        }
        positions.push(IndexArg { pos, proven });
    }
    Some((positions, params.clone()))
}

/// Check mode (`PERRY_VERIFY_FACTS=1`): the argument the proof covers is an
/// exact int32 (a plain double, never `-0`) and not negative (fact `index
/// argument`).
fn witness_nonneg(ctx: &mut FnCtx<'_>, value: &str, proof: Proof) {
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::IndexArgument,
        || serde_json::json!({ "proof": format!("{proof:?}") }),
    );
    crate::facts_consumed::used();
    if !crate::verify_facts::enabled() {
        return;
    }
    let blk = ctx.block();
    // A Number is a plain double: the witness admits an exact int32 double.
    let bits = blk.bitcast_double_to_i64(value);
    let (exact, int) = crate::codegen::emit_typed_i32_plain_guard_and_raw(blk, value, &bits);
    let nonnegative = blk.icmp_sge(I32, &int, "0");
    let ok = blk.and(I1, &exact, &nonnegative);
    crate::verify_facts::tick(ctx);
    let miss = ctx.new_block("index.arg.verify_miss");
    let next = ctx.new_block("index.arg.admitted");
    let miss_l = ctx.block_label(miss);
    let next_l = ctx.block_label(next);
    ctx.block().cond_br(&ok, &next_l, &miss_l);
    ctx.current_block = miss;
    crate::verify_facts::covered(
        crate::verify_facts::Fact::IndexArgument,
        || serde_json::json!({ "proof": format!("{proof:?}") }),
    );
    let site = crate::verify_facts::site_arg(
        ctx,
        &format!("index argument {proof:?}"),
        crate::verify_facts::Fact::IndexArgument,
        &[],
    );
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(crate::types::DOUBLE, value)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
}

/// The i32 of a proven non-negative int32 local: its canonical i32 slot, or
/// its double (an exact int32, `proven_number_raw`).
fn proven_raw(ctx: &mut FnCtx<'_>, id: u32, value: &str) -> String {
    if let Some((slot, _)) = crate::expr::canonical_local_i32_slot(ctx, id) {
        return ctx.block().load(I32, &slot);
    }
    proven_number_raw(ctx, value)
}

/// The i32 of `value`, a Number the facts prove an int32: a plain double, so
/// one `fptosi`.
fn proven_number_raw(ctx: &mut FnCtx<'_>, value: &str) -> String {
    ctx.block().fptosi(crate::types::DOUBLE, value, I32)
}

/// Call `target`'s `$idx_u31` clone when every selected argument is a
/// nonnegative i32, its `$generic` body otherwise. `arg_slices[0]` is the
/// receiver; user argument `k` is `arg_slices[k + 1]`. An argument the
/// interval proof covers (`IndexArg::proven`) needs no test; when every one
/// is covered the call goes to the clone's raw body with no split.
pub(crate) fn emit_number_index_split(
    ctx: &mut FnCtx<'_>,
    target: &str,
    positions: &[IndexArg],
    params: &[u32],
    arg_slices: &[(LlvmType, &str)],
) -> String {
    let mut admitted: Option<String> = None;
    let mut raws: Vec<String> = Vec::with_capacity(positions.len());
    for arg in positions {
        let value = arg_slices[arg.pos + 1].1.to_string();
        if let Some(proof) = arg.proven {
            witness_nonneg(ctx, &value, proof);
            let raw = match proof {
                Proof::Local(id) | Proof::Site(Some(id)) => proven_raw(ctx, id, &value),
                Proof::Site(None) => proven_number_raw(ctx, &value),
            };
            raws.push(raw);
            continue;
        }
        let blk = ctx.block();
        // The public entry's own predicate: both admit the same set, and
        // `-0` is excluded because the clone reads its index back as `+0`.
        let bits = blk.bitcast_double_to_i64(&value);
        let (exact, int) = crate::codegen::emit_typed_i32_plain_guard_and_raw(blk, &value, &bits);
        let nonnegative = blk.icmp_sge(I32, &int, "0");
        raws.push(int);
        let ok = blk.and(I1, &exact, &nonnegative);
        admitted = Some(match admitted {
            Some(prev) => blk.and(I1, &prev, &ok),
            None => ok,
        });
    }
    let idx_fn = crate::codegen::nonnegative_index_method_name(target, params);
    let index_positions: Vec<usize> = positions.iter().map(|a| a.pos).collect();
    let raw_call = |ctx: &mut FnCtx<'_>| -> String {
        // The clone's raw-i32 body takes the proved i32s.
        if crate::codegen::index_entry::raw_i32_enabled() {
            let raw_fn = crate::codegen::index_entry::raw_body_name(&idx_fn);
            let raw_args =
                crate::codegen::index_entry::raw_body_args(arg_slices, &index_positions, &raws);
            ctx.block().call(DOUBLE, &raw_fn, &raw_args)
        } else {
            ctx.block().call(DOUBLE, &idx_fn, arg_slices)
        }
    };
    let Some(admitted) = admitted else {
        // Every index argument is interval-proven.
        return raw_call(ctx);
    };
    let generic_fn = crate::codegen::generic_method_body_name(target);
    let fast_idx = ctx.new_block("number_index.fast");
    let generic_idx = ctx.new_block("number_index.generic");
    let merge_idx = ctx.new_block("number_index.merge");
    let fast_label = ctx.block_label(fast_idx);
    let generic_label = ctx.block_label(generic_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&admitted, &fast_label, &generic_label);
    ctx.current_block = fast_idx;
    let fast = raw_call(ctx);
    let fast_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);
    ctx.current_block = generic_idx;
    let slow = ctx.block().call(DOUBLE, &generic_fn, arg_slices);
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);
    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[(fast.as_str(), &fast_end), (slow.as_str(), &slow_end)],
    )
}

#[cfg(test)]
#[path = "index_split_tests.rs"]
mod tests;
