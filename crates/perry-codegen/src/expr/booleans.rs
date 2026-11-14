//! Booleans (wpfacts `booleans`): the truthiness of a value the
//! whole-program facts prove `true` or `false`.
//!
//! - `ElisionFacts::boolean_locals`: every value the local holds is a
//!   Boolean;
//! - `ElisionFacts::boolean_fields` (`FieldProof::booleans`): every store
//!   of the program puts a Boolean into the field, and the read is the
//!   guard-free load of a definite birth field (no absent field, no other
//!   receiver, no interpreted writer).
//!
//! The lowered word is then `TAG_TRUE` or `TAG_FALSE`, and its truthiness
//! is `bits == TAG_TRUE`: the generic test's number arm, tag range test,
//! pointer test and `js_is_truthy` call are unreachable. A declared
//! `boolean` proves nothing of this (`lower_conditional::lower_truthy`,
//! #7846): the fact is about the values the program stores.
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`): the word is one of the two tags, or
//! the facts are false (`perry_wpfacts_check_miss`, "boolean", exit 86).
//! `PERRY_FACTS_BOOLEANS=0` turns the fact and this lowering off.

use perry_hir::Expr;

use crate::types::{DOUBLE, I1, I32, I64};

use super::FnCtx;

fn usable(ctx: &FnCtx<'_>) -> bool {
    !ctx.wpfacts_off && !ctx.wpfacts.interp_guard && !super::typed_feedback_emission_enabled()
}

/// What proves `expr` a Boolean, for the check-mode report: a local of
/// `boolean_locals`, or a guard-free read of a field of `boolean_fields`.
fn proven(ctx: &FnCtx<'_>, expr: &Expr) -> Option<String> {
    if !usable(ctx) {
        return None;
    }
    match expr {
        Expr::LocalGet(id) => ctx
            .wpfacts
            .boolean_locals
            .contains(id)
            .then(|| format!("local {id} holds booleans")),
        Expr::PropertyGet {
            object, property, ..
        } => {
            // Only a receiver that can have a field proof at all is asked.
            if ctx.wpfacts.boolean_fields.is_empty()
                || !matches!(object.as_ref(), Expr::LocalGet(_) | Expr::This)
            {
                return None;
            }
            let proof = super::wpfacts_access::prove(
                ctx,
                object,
                property,
                super::wpfacts_access::Access::Get,
            )?;
            (proof.booleans && !proof.guarded()).then(|| format!("field {property} holds booleans"))
        }
        _ => None,
    }
}

/// The truthiness of `value`, the lowering of `expr`, when the facts prove
/// `expr` a Boolean (module docs): `bits == TAG_TRUE` as an `i1`.
pub(crate) fn truthy(ctx: &mut FnCtx<'_>, value: &str, expr: &Expr) -> Option<String> {
    let what = proven(ctx, expr)?;
    crate::facts_consumed::used();
    crate::verify_facts::covered(
        crate::verify_facts::Fact::Boolean,
        || serde_json::json!({ "truthy": what }),
    );
    let bits = ctx.block().bitcast_double_to_i64(value);
    witness(ctx, value, &bits, &what);
    Some(ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64))
}

/// Check mode: `bits` is `TAG_TRUE` or `TAG_FALSE`.
fn witness(ctx: &mut FnCtx<'_>, value: &str, bits: &str, what: &str) {
    if !crate::verify_facts::enabled() {
        return;
    }
    let miss = ctx.new_block("bool.verify_miss");
    let next = ctx.new_block("bool.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    crate::verify_facts::tick(ctx);
    let blk = ctx.block();
    let is_true = blk.icmp_eq(I64, bits, crate::nanbox::TAG_TRUE_I64);
    let is_false = blk.icmp_eq(I64, bits, crate::nanbox::TAG_FALSE_I64);
    let boolean = blk.or(I1, &is_true, &is_false);
    blk.cond_br(&boolean, &next_l, &miss_l);
    ctx.current_block = miss;
    let site = crate::verify_facts::site_arg(ctx, what, crate::verify_facts::Fact::Boolean, &[]);
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

#[cfg(test)]
#[path = "booleans_tests.rs"]
mod tests;
