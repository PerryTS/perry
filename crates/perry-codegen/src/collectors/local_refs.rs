//! Local-ref scanning helpers used by escape analysis.
//!
//! Split out of `escape_news.rs` in v0.5.1021 to satisfy the file-size CI
//! gate. No behavior change — these functions remain `pub` and are re-
//! exported from `collectors/mod.rs`.

use std::collections::HashSet;

use super::*;

/// Helper: does this expression contain `LocalGet(target_id)` anywhere?
pub fn expr_contains_local_get(e: &perry_hir::Expr, target_id: u32) -> bool {
    use perry_hir::Expr;
    match e {
        Expr::LocalGet(id) => *id == target_id,
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            expr_contains_local_get(left, target_id) || expr_contains_local_get(right, target_id)
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Await(operand)
        | Expr::StringCoerce(operand)
        | Expr::NumberCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::Delete(operand) => expr_contains_local_get(operand, target_id),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_contains_local_get(condition, target_id)
                || expr_contains_local_get(then_expr, target_id)
                || expr_contains_local_get(else_expr, target_id)
        }
        Expr::Call { callee, args, .. } => {
            expr_contains_local_get(callee, target_id)
                || args.iter().any(|a| expr_contains_local_get(a, target_id))
        }
        Expr::PropertyGet { object, .. } | Expr::PropertyUpdate { object, .. } => {
            expr_contains_local_get(object, target_id)
        }
        Expr::PropertySet { object, value, .. } => {
            expr_contains_local_get(object, target_id) || expr_contains_local_get(value, target_id)
        }
        Expr::IndexGet { object, index } => {
            expr_contains_local_get(object, target_id) || expr_contains_local_get(index, target_id)
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            expr_contains_local_get(object, target_id)
                || expr_contains_local_get(index, target_id)
                || expr_contains_local_get(value, target_id)
        }
        Expr::LocalSet(_, value) => expr_contains_local_get(value, target_id),
        Expr::Array(elements) => elements
            .iter()
            .any(|e| expr_contains_local_get(e, target_id)),
        Expr::Object(props) => props
            .iter()
            .any(|(_, v)| expr_contains_local_get(v, target_id)),
        Expr::New { args, .. } => args.iter().any(|a| expr_contains_local_get(a, target_id)),
        Expr::Sequence(es) => es.iter().any(|e| expr_contains_local_get(e, target_id)),
        Expr::Update { id, .. } => *id == target_id,
        _ => false, // Conservative: we don't recurse into everything, but false means "not found" which is safe
    }
}

/// Conservative catch-all: walk the expression and mark any candidate
/// local referenced via LocalGet as escaped. Used for Expr variants we
/// haven't explicitly enumerated in check_escapes_in_expr.
///
/// **Safety note (issue #150):** `collect_ref_ids_in_expr` has a silent
/// `_ => {}` fallthrough for unenumerated HIR variants. That means for
/// variants like `ObjectGetOwnPropertyDescriptor(LocalGet(p), key)` — which
/// is an identity-observing operation that should escape `p` — the collector
/// returns an empty set, and `p` ends up scalar-replaced while an external
/// runtime function (`js_object_get_own_property_descriptor`) tries to
/// dereference its dummy alloca slot. Since we can't enumerate every HIR
/// variant that might embed a LocalGet, we conservatively mark EVERY
/// candidate as escaped whenever this catch-all fires. The cost is losing
/// scalar replacement in functions that happen to contain an un-enumerated
/// variant anywhere; the safety is not silently miscompiling identity-
/// observing code. This mirrors the `check_object_literal_escapes_in_expr`
/// catch-all at line ~4148 which already does exactly this for object
/// literal candidates.
pub fn mark_all_candidate_refs_in_expr(
    e: &perry_hir::Expr,
    candidates: &std::collections::HashMap<u32, String>,
    escaped: &mut HashSet<u32>,
) {
    // First pass: walk what collect_ref_ids_in_expr knows about — these are
    // the references we can prove exist.
    let mut refs: HashSet<u32> = HashSet::new();
    collect_ref_ids_in_expr(e, &mut refs);
    for id in refs {
        if candidates.contains_key(&id) {
            escaped.insert(id);
        }
    }
    // Second pass: conservative fallback. We're in the check_escapes_in_expr
    // catch-all, meaning `e` is some HIR variant not explicitly enumerated
    // there. The collector above may have silently skipped unknown
    // sub-variants, so we must assume any candidate in scope could be
    // referenced transitively. Mark them all escaped.
    for id in candidates.keys() {
        escaped.insert(*id);
    }
}
