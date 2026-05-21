use perry_hir::{BinaryOp, Expr, Function, Stmt};
use std::collections::HashSet;

use super::*;

pub fn collect_non_escaping_arrays(
    stmts: &[perry_hir::Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &std::collections::HashMap<u32, String>,
) -> std::collections::HashMap<u32, u32> {
    let mut candidates: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    find_array_candidates(stmts, boxed_vars, module_globals, &mut candidates);

    if candidates.is_empty() {
        return candidates;
    }

    let mut escaped: HashSet<u32> = HashSet::new();
    check_array_escapes_in_stmts(stmts, &candidates, &mut escaped);

    candidates.retain(|id, _| !escaped.contains(id));
    candidates
}

pub fn find_array_candidates(
    stmts: &[perry_hir::Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &std::collections::HashMap<u32, String>,
    candidates: &mut std::collections::HashMap<u32, u32>,
) {
    use perry_hir::{Expr, Stmt};
    for s in stmts {
        match s {
            Stmt::Let {
                id,
                init: Some(Expr::Array(elements)),
                ..
            } => {
                if !boxed_vars.contains(id) && !module_globals.contains_key(id) {
                    let n = elements.len();
                    if (1..=MAX_SCALAR_ARRAY_LEN).contains(&n) {
                        candidates.insert(*id, n as u32);
                    }
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                find_array_candidates(then_branch, boxed_vars, module_globals, candidates);
                if let Some(eb) = else_branch {
                    find_array_candidates(eb, boxed_vars, module_globals, candidates);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    find_array_candidates(
                        std::slice::from_ref(init_stmt),
                        boxed_vars,
                        module_globals,
                        candidates,
                    );
                }
                find_array_candidates(body, boxed_vars, module_globals, candidates);
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                find_array_candidates(body, boxed_vars, module_globals, candidates);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                find_array_candidates(body, boxed_vars, module_globals, candidates);
                if let Some(c) = catch {
                    find_array_candidates(&c.body, boxed_vars, module_globals, candidates);
                }
                if let Some(f) = finally {
                    find_array_candidates(f, boxed_vars, module_globals, candidates);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    find_array_candidates(&c.body, boxed_vars, module_globals, candidates);
                }
            }
            Stmt::Labeled { body, .. } => {
                find_array_candidates(
                    std::slice::from_ref(body.as_ref()),
                    boxed_vars,
                    module_globals,
                    candidates,
                );
            }
            _ => {}
        }
    }
}

pub fn check_array_escapes_in_stmts(
    stmts: &[perry_hir::Stmt],
    candidates: &std::collections::HashMap<u32, u32>,
    escaped: &mut HashSet<u32>,
) {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) => check_array_escapes_in_expr(e, candidates, escaped),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    check_array_escapes_in_expr(e, candidates, escaped);
                }
            }
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    check_array_escapes_in_expr(e, candidates, escaped);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_array_escapes_in_expr(condition, candidates, escaped);
                check_array_escapes_in_stmts(then_branch, candidates, escaped);
                if let Some(eb) = else_branch {
                    check_array_escapes_in_stmts(eb, candidates, escaped);
                }
            }
            Stmt::While { condition, body } => {
                check_array_escapes_in_expr(condition, candidates, escaped);
                check_array_escapes_in_stmts(body, candidates, escaped);
            }
            Stmt::DoWhile { body, condition } => {
                check_array_escapes_in_stmts(body, candidates, escaped);
                check_array_escapes_in_expr(condition, candidates, escaped);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    check_array_escapes_in_stmts(
                        std::slice::from_ref(init_stmt),
                        candidates,
                        escaped,
                    );
                }
                if let Some(cond) = condition {
                    check_array_escapes_in_expr(cond, candidates, escaped);
                }
                if let Some(upd) = update {
                    check_array_escapes_in_expr(upd, candidates, escaped);
                }
                check_array_escapes_in_stmts(body, candidates, escaped);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                check_array_escapes_in_expr(discriminant, candidates, escaped);
                for case in cases {
                    if let Some(test) = &case.test {
                        check_array_escapes_in_expr(test, candidates, escaped);
                    }
                    check_array_escapes_in_stmts(&case.body, candidates, escaped);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                check_array_escapes_in_stmts(body, candidates, escaped);
                if let Some(c) = catch {
                    check_array_escapes_in_stmts(&c.body, candidates, escaped);
                }
                if let Some(f) = finally {
                    check_array_escapes_in_stmts(f, candidates, escaped);
                }
            }
            Stmt::Labeled { body, .. } => {
                check_array_escapes_in_stmts(
                    std::slice::from_ref(body.as_ref()),
                    candidates,
                    escaped,
                );
            }
            _ => {}
        }
    }
}

/// Extract a non-negative integer from an index expression if and only if it's
/// a compile-time literal that fits in u32. `Integer(k)` and `Number(k)`
/// (when `k` is an exact integer) both count.
pub fn const_index(expr: &perry_hir::Expr) -> Option<u32> {
    use perry_hir::Expr;
    match expr {
        Expr::Integer(k) if *k >= 0 && *k <= u32::MAX as i64 => Some(*k as u32),
        Expr::Number(f)
            if f.is_finite() && *f >= 0.0 && f.fract() == 0.0 && *f <= u32::MAX as f64 =>
        {
            Some(*f as u32)
        }
        _ => None,
    }
}

pub fn check_array_escapes_in_expr(
    e: &perry_hir::Expr,
    candidates: &std::collections::HashMap<u32, u32>,
    escaped: &mut HashSet<u32>,
) {
    use perry_hir::{ArrayElement, CallArg, Expr};

    match e {
        // Safe: constant-index read `arr[k]` where 0 <= k < length.
        Expr::IndexGet { object, index } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(&len) = candidates.get(id) {
                    match const_index(index) {
                        Some(k) if k < len => {
                            // Safe use — walk index for other candidates (none
                            // in a literal), skip object walk.
                            check_array_escapes_in_expr(index, candidates, escaped);
                            return;
                        }
                        _ => {
                            // Dynamic or out-of-range index: must keep real array.
                            escaped.insert(*id);
                        }
                    }
                }
            }
            check_array_escapes_in_expr(object, candidates, escaped);
            check_array_escapes_in_expr(index, candidates, escaped);
        }

        // Safe: `arr.length` read folds to the constant N.
        Expr::PropertyGet { object, property } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if candidates.contains_key(id) && property == "length" {
                    return;
                }
            }
            check_array_escapes_in_expr(object, candidates, escaped);
        }

        // IndexSet would mutate the array — treat as escape. (Supporting this
        // would require tracking dirty slots and invalidating earlier reads;
        // not worth the complexity for literals that are mostly read-only.)
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if candidates.contains_key(id) {
                    escaped.insert(*id);
                }
            }
            check_array_escapes_in_expr(object, candidates, escaped);
            check_array_escapes_in_expr(index, candidates, escaped);
            check_array_escapes_in_expr(value, candidates, escaped);
        }

        Expr::IndexUpdate { object, index, .. } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if candidates.contains_key(id) {
                    escaped.insert(*id);
                }
            }
            check_array_escapes_in_expr(object, candidates, escaped);
            check_array_escapes_in_expr(index, candidates, escaped);
        }

        // Reassignment is always an escape (and any LocalGet anywhere else).
        Expr::LocalSet(id, value) => {
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
            check_array_escapes_in_expr(value, candidates, escaped);
        }
        Expr::LocalGet(id) => {
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
        }

        // Closure captures: if a candidate is captured, it escapes.
        Expr::Closure { body, captures, .. } => {
            for c in captures {
                if candidates.contains_key(c) {
                    escaped.insert(*c);
                }
            }
            check_array_escapes_in_stmts(body, candidates, escaped);
        }

        // ── Recurse into sub-expressions (same structure as object pass). ──
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            check_array_escapes_in_expr(left, candidates, escaped);
            check_array_escapes_in_expr(right, candidates, escaped);
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Await(operand)
        | Expr::Delete(operand)
        | Expr::StringCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand)
        | Expr::IsFinite(operand)
        | Expr::IsNaN(operand)
        | Expr::NumberIsNaN(operand)
        | Expr::NumberIsFinite(operand)
        | Expr::NumberIsInteger(operand)
        | Expr::IsUndefinedOrBareNan(operand)
        | Expr::ParseFloat(operand) => {
            check_array_escapes_in_expr(operand, candidates, escaped);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            check_array_escapes_in_expr(condition, candidates, escaped);
            check_array_escapes_in_expr(then_expr, candidates, escaped);
            check_array_escapes_in_expr(else_expr, candidates, escaped);
        }
        Expr::Call { callee, args, .. } => {
            check_array_escapes_in_expr(callee, candidates, escaped);
            for a in args {
                check_array_escapes_in_expr(a, candidates, escaped);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            check_array_escapes_in_expr(callee, candidates, escaped);
            for a in args {
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => {
                        check_array_escapes_in_expr(e, candidates, escaped);
                    }
                }
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                check_array_escapes_in_expr(o, candidates, escaped);
            }
            for a in args {
                check_array_escapes_in_expr(a, candidates, escaped);
            }
        }
        Expr::Array(elements) => {
            for el in elements {
                check_array_escapes_in_expr(el, candidates, escaped);
            }
        }
        Expr::ArraySpread(elements) => {
            for el in elements {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                        check_array_escapes_in_expr(e, candidates, escaped);
                    }
                }
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                check_array_escapes_in_expr(v, candidates, escaped);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                check_array_escapes_in_expr(a, candidates, escaped);
            }
        }
        Expr::PropertySet { object, value, .. } => {
            check_array_escapes_in_expr(object, candidates, escaped);
            check_array_escapes_in_expr(value, candidates, escaped);
        }
        Expr::PropertyUpdate { object, .. } => {
            check_array_escapes_in_expr(object, candidates, escaped);
        }
        Expr::Sequence(es) => {
            for e in es {
                check_array_escapes_in_expr(e, candidates, escaped);
            }
        }
        Expr::Update { id, .. } => {
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
        }
        // Leaf expressions: no LocalGet inside.
        Expr::Integer(_)
        | Expr::Number(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Undefined
        | Expr::Null
        | Expr::This
        | Expr::FuncRef(_)
        | Expr::ClassRef(_)
        | Expr::ExternFuncRef { .. }
        | Expr::GlobalGet(_)
        | Expr::BigInt(_) => {}
        // Catch-all: any unrecognized expression conservatively marks every
        // candidate it references as escaped. Safe — we just miss the
        // optimization on patterns we haven't enumerated above.
        _ => {
            let mut refs: HashSet<u32> = HashSet::new();
            collect_ref_ids_in_expr(e, &mut refs);
            for id in refs {
                if candidates.contains_key(&id) {
                    escaped.insert(id);
                }
            }
        }
    }
}

// ── Escape analysis for scalar replacement of non-escaping object literals ──

/// Upper bound on field count — matches `MAX_SCALAR_ARRAY_LEN`. Beyond this the
/// per-field alloca cost overtakes the arena-bump heap path we'd otherwise use.
pub(crate) const MAX_SCALAR_OBJECT_FIELDS: usize = 16;
