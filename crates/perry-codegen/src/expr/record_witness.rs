//! Check mode for value records (DESIGN-p4l4 B7, L4; VR-2).
//!
//! Under `PERRY_VERIFY_FACTS=1` the facts' admitted record templates
//! (`ElisionFacts::record_classes`, filled only with
//! `PERRY_FACTS_VALUE_RECORDS=1`) are claimed at birth
//! (`perry_verify_facts_record_claim`), and every compiled identity
//! comparison (`===`, `!==`, `==`, `!=` between non-literal operands, each
//! `switch` case) hands both operands to `perry_verify_facts_record_identity`,
//! which aborts with rc 86 (`Fact::RecordIdentity`) on a claimed one: the
//! facts said no such comparison exists (R4). Release code and a program
//! without admitted records emit nothing.
use super::FnCtx;
use crate::types::{DOUBLE, I32};
use crate::verify_facts::Fact;

/// The witness is emitted in this compilation.
pub(crate) fn active(ctx: &FnCtx<'_>) -> bool {
    crate::verify_facts::enabled() && !ctx.wpfacts.record_classes.is_empty()
}

/// The identity witness of one compiled comparison of `a` and `b` (boxed
/// words).
pub(crate) fn identity(ctx: &mut FnCtx<'_>, a: &str, b: &str, what: &str) {
    if !active(ctx) {
        return;
    }
    crate::verify_facts::covered(Fact::RecordIdentity, || serde_json::json!({ "what": what }));
    let site = crate::verify_facts::site_arg(ctx, what, Fact::RecordIdentity, &[]);
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_record_identity",
        &[(DOUBLE, a), (DOUBLE, b)],
        &site,
    );
}

/// The witness of a compiled `===`, `!==`, `==` or `!=` of `left` and
/// `right` (lowered to `l`, `r`): an identity comparison unless an operand
/// is a primitive literal (a presence or value test; wpfacts
/// `gen_expr::primitive_literal`).
pub(crate) fn compare(
    ctx: &mut FnCtx<'_>,
    op: &perry_hir::CompareOp,
    left: &perry_hir::Expr,
    right: &perry_hir::Expr,
    l: &str,
    r: &str,
) {
    use perry_hir::{CompareOp, Expr};
    let literal = |e: &Expr| {
        matches!(
            e,
            Expr::Undefined
                | Expr::Null
                | Expr::Bool(_)
                | Expr::Number(_)
                | Expr::Integer(_)
                | Expr::BigInt(_)
                | Expr::String(_)
                | Expr::WtfString(_)
        )
    };
    if matches!(
        op,
        CompareOp::Eq | CompareOp::Ne | CompareOp::LooseEq | CompareOp::LooseNe
    ) && !literal(left)
        && !literal(right)
    {
        identity(ctx, l, r, "compiled identity compare");
    }
}

/// Claim `value`, a fresh instance of class `class_name`, when its template
/// is an admitted value record.
pub(crate) fn claim(ctx: &mut FnCtx<'_>, class_name: &str, value: &str) {
    if !active(ctx) {
        return;
    }
    let Some(&cid) = ctx.class_ids.get(class_name) else {
        return;
    };
    if ctx.wpfacts.record_classes.contains(&cid) {
        ctx.block()
            .call_void("perry_verify_facts_record_claim", &[(DOUBLE, value)]);
    }
}
