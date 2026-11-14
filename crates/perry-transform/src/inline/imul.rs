use perry_hir::types::{FuncId, LocalId};
use perry_hir::{BinaryOp, Expr, Function, Stmt};
use std::collections::HashSet;

pub fn detect_math_imul_polyfill(f: &Function) -> bool {
    if f.is_async || f.is_generator {
        return false;
    }
    if f.params.len() != 2 {
        return false;
    }
    if f.body.len() != 5 {
        return false;
    }

    let p0 = f.params[0].id;
    let p1 = f.params[1].id;

    // First 4 stmts must be immutable Lets with half-word extraction inits
    let mut hi_of = [false; 2]; // hi_of[0] = saw hi-half of p0, hi_of[1] = p1
    let mut lo_of = [false; 2];
    for stmt in &f.body[..4] {
        match stmt {
            Stmt::Let {
                mutable: false,
                init: Some(init),
                ..
            } => {
                if let Some((pid, is_hi)) = is_half_extract(init, p0, p1) {
                    let idx = if pid == p0 { 0 } else { 1 };
                    if is_hi {
                        hi_of[idx] = true;
                    } else {
                        lo_of[idx] = true;
                    }
                } else {
                    return false;
                }
            }
            _ => return false,
        }
    }
    if !(hi_of[0] && lo_of[0] && hi_of[1] && lo_of[1]) {
        return false;
    }

    // Last stmt: Return(Some(Binary { BitOr, ..., Integer(0) }))
    matches!(&f.body[4], Stmt::Return(Some(Expr::Binary { op: BinaryOp::BitOr, right, .. })) if matches!(right.as_ref(), Expr::Integer(0)))
}

/// Check if an expression extracts the hi or lo 16-bit half of a parameter.
/// Returns `Some((param_id, is_hi))` on match.
pub fn is_half_extract(e: &Expr, p0: LocalId, p1: LocalId) -> Option<(LocalId, bool)> {
    // Pattern: (param >>> 16) & 0xffff  OR  (param >> 16) & 0xffff  →  hi-half
    // Pattern: param & 0xffff  →  lo-half
    match e {
        Expr::Binary {
            op: BinaryOp::BitAnd,
            left,
            right,
        } => {
            if !matches!(right.as_ref(), Expr::Integer(0xffff)) {
                return None;
            }
            match left.as_ref() {
                Expr::Binary {
                    op: BinaryOp::UShr | BinaryOp::Shr,
                    left: inner,
                    right: shift_amt,
                } => {
                    if !matches!(shift_amt.as_ref(), Expr::Integer(16)) {
                        return None;
                    }
                    match inner.as_ref() {
                        Expr::LocalGet(id) if *id == p0 || *id == p1 => Some((*id, true)),
                        _ => None,
                    }
                }
                Expr::LocalGet(id) if *id == p0 || *id == p1 => Some((*id, false)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Rewrite `Call(FuncRef(imul_id), [a, b])` → `MathImul(a, b)` in statements.
/// Every statement kind is visited (labelled loops, switch cases, try
/// blocks included): which statement holds the call must not decide whether
/// it becomes `MathImul`.
pub fn rewrite_imul_calls_in_stmts(stmts: &mut [Stmt], imul_ids: &HashSet<FuncId>) {
    for s in stmts.iter_mut() {
        crate::closure_local_inline::for_each_expr_in_stmt_mut(s, &mut |e| {
            rewrite_imul_calls_in_expr(e, imul_ids)
        });
        for list in crate::closure_local_inline::nested_stmt_lists(s) {
            rewrite_imul_calls_in_stmts(list, imul_ids);
        }
    }
}

pub fn rewrite_imul_calls_in_expr(e: &mut Expr, imul_ids: &HashSet<FuncId>) {
    // Check if this expr is a call to an imul polyfill
    let is_imul = matches!(e, Expr::Call { callee, args, .. }
        if args.len() == 2 && matches!(callee.as_ref(), Expr::FuncRef(fid) if imul_ids.contains(fid)));
    if is_imul {
        if let Expr::Call { args, .. } = std::mem::replace(e, Expr::Undefined) {
            let mut args = args;
            let b = args.pop().unwrap();
            let a = args.pop().unwrap();
            *e = Expr::MathImul(Box::new(a), Box::new(b));
        }
        // Recurse into the new MathImul operands
        if let Expr::MathImul(a, b) = e {
            rewrite_imul_calls_in_expr(a, imul_ids);
            rewrite_imul_calls_in_expr(b, imul_ids);
        }
        return;
    }

    // Recurse into sub-expressions
    match e {
        Expr::Binary { left, right, .. }
        | Expr::Logical { left, right, .. }
        | Expr::Compare { left, right, .. } => {
            rewrite_imul_calls_in_expr(left, imul_ids);
            rewrite_imul_calls_in_expr(right, imul_ids);
        }
        Expr::Unary { operand, .. } => rewrite_imul_calls_in_expr(operand, imul_ids),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            rewrite_imul_calls_in_expr(condition, imul_ids);
            rewrite_imul_calls_in_expr(then_expr, imul_ids);
            rewrite_imul_calls_in_expr(else_expr, imul_ids);
        }
        Expr::Call { callee, args, .. } => {
            rewrite_imul_calls_in_expr(callee, imul_ids);
            for arg in args {
                rewrite_imul_calls_in_expr(arg, imul_ids);
            }
        }
        Expr::LocalSet(_, val) => rewrite_imul_calls_in_expr(val, imul_ids),
        Expr::IndexGet { object, index, .. } => {
            rewrite_imul_calls_in_expr(object, imul_ids);
            rewrite_imul_calls_in_expr(index, imul_ids);
        }
        Expr::IndexSet {
            object,
            index,
            value,
            ..
        } => {
            rewrite_imul_calls_in_expr(object, imul_ids);
            rewrite_imul_calls_in_expr(index, imul_ids);
            rewrite_imul_calls_in_expr(value, imul_ids);
        }
        Expr::Array(elems, _) => {
            for el in elems {
                rewrite_imul_calls_in_expr(el, imul_ids);
            }
        }
        Expr::PropertyGet { object, .. } => rewrite_imul_calls_in_expr(object, imul_ids),
        Expr::PropertySet { object, value, .. } => {
            rewrite_imul_calls_in_expr(object, imul_ids);
            rewrite_imul_calls_in_expr(value, imul_ids);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::SwitchCase;

    const IMUL: FuncId = 7;

    fn imul_call() -> Stmt {
        Stmt::Expr(Expr::LocalSet(
            1,
            Box::new(Expr::Call {
                callee: Box::new(Expr::FuncRef(IMUL)),
                args: vec![Expr::LocalGet(1), Expr::Integer(16777619)],
                type_args: Vec::new(),
                byte_offset: 0,
                byte_end: 0,
            }),
        ))
    }

    fn counted_loop(body: Vec<Stmt>) -> Stmt {
        Stmt::For {
            init: None,
            condition: Some(Expr::Bool(false)),
            update: None,
            body,
        }
    }

    fn is_math_imul(stmt: &Stmt) -> bool {
        matches!(stmt, Stmt::Expr(Expr::LocalSet(1, value)) if matches!(**value, Expr::MathImul(..)))
    }

    /// A label (or a switch case) around the call must not keep the
    /// polyfill call: the same loop unlabelled gets `MathImul`.
    #[test]
    fn a_labelled_loop_and_a_switch_case_get_math_imul_like_a_plain_loop() {
        let ids: HashSet<FuncId> = [IMUL].into_iter().collect();
        let mut stmts = vec![
            counted_loop(vec![imul_call()]),
            Stmt::Labeled {
                label: "fnv".to_string(),
                body: Box::new(counted_loop(vec![imul_call()])),
            },
            Stmt::Switch {
                discriminant: Expr::Integer(0),
                cases: vec![SwitchCase {
                    test: None,
                    body: vec![imul_call()],
                }],
            },
        ];
        rewrite_imul_calls_in_stmts(&mut stmts, &ids);
        let Stmt::For { body: plain, .. } = &stmts[0] else {
            panic!("plain loop")
        };
        assert!(is_math_imul(&plain[0]), "plain loop");
        let Stmt::Labeled { body, .. } = &stmts[1] else {
            panic!("labelled loop")
        };
        let Stmt::For { body: labelled, .. } = body.as_ref() else {
            panic!("labelled for")
        };
        assert!(
            is_math_imul(&labelled[0]),
            "labelled loop keeps the polyfill call"
        );
        let Stmt::Switch { cases, .. } = &stmts[2] else {
            panic!("switch")
        };
        assert!(
            is_math_imul(&cases[0].body[0]),
            "switch case keeps the polyfill call"
        );
    }
}
