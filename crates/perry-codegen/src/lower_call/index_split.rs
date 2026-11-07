//! A call of an indexed method (`$idx_u31` clone) whose index arguments are
//! proven Numbers.
//!
//! The public entry of such a method guards a boxed argument with the exact
//! i32 test: an INT32 box or a double that converts exactly, then a sign
//! test. A call site that proves the argument a canonical double
//! (`expr_produces_canonical_raw_f64`) needs only the double half: the
//! saturating conversion round-trips exactly for every in-range integral
//! value (`-0` converts to `0`, the same array index) and fails for NaN, a
//! fraction or a value out of the i32 range. The site therefore selects the
//! clone or the generic body itself, and the public guard is not emitted.
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

/// The argument positions of `class_name.property`'s index parameters when
/// the method has an `$idx_u31` clone and every such argument is a proven
/// canonical double, with the clone's parameter-id list.
pub(crate) fn number_index_positions(
    ctx: &FnCtx<'_>,
    class_name: &str,
    property: &str,
    args: &[Expr],
) -> Option<(Vec<usize>, Vec<u32>)> {
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
        if !crate::type_analysis::expr_produces_canonical_raw_f64(ctx, args.get(pos)?) {
            return None;
        }
        positions.push(pos);
    }
    Some((positions, params.clone()))
}

/// Call `target`'s `$idx_u31` clone when every selected argument is a
/// nonnegative i32, its `$generic` body otherwise. `arg_slices[0]` is the
/// receiver; user argument `k` is `arg_slices[k + 1]`.
pub(crate) fn emit_number_index_split(
    ctx: &mut FnCtx<'_>,
    target: &str,
    positions: &[usize],
    params: &[u32],
    arg_slices: &[(LlvmType, &str)],
) -> String {
    let mut admitted: Option<String> = None;
    for pos in positions {
        let value = arg_slices[pos + 1].1.to_string();
        let blk = ctx.block();
        let int = blk.call(I32, "llvm.fptosi.sat.i32.f64", &[(DOUBLE, &value)]);
        let back = blk.sitofp(I32, &int, DOUBLE);
        let exact = blk.fcmp("oeq", &back, &value);
        let nonnegative = blk.icmp_sge(I32, &int, "0");
        let ok = blk.and(I1, &exact, &nonnegative);
        admitted = Some(match admitted {
            Some(prev) => blk.and(I1, &prev, &ok),
            None => ok,
        });
    }
    let admitted = admitted.expect("an indexed method has an index parameter");
    let idx_fn = crate::codegen::nonnegative_index_method_name(target, params);
    let generic_fn = crate::codegen::generic_method_body_name(target);
    let fast_idx = ctx.new_block("number_index.fast");
    let generic_idx = ctx.new_block("number_index.generic");
    let merge_idx = ctx.new_block("number_index.merge");
    let fast_label = ctx.block_label(fast_idx);
    let generic_label = ctx.block_label(generic_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&admitted, &fast_label, &generic_label);
    ctx.current_block = fast_idx;
    let fast = ctx.block().call(DOUBLE, &idx_fn, arg_slices);
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
