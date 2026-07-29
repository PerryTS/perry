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
pub fn rewrite_imul_calls_in_stmts(stmts: &mut [Stmt], imul_ids: &HashSet<FuncId>) {
    for s in stmts.iter_mut() {
        match s {
            Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => {
                rewrite_imul_calls_in_expr(e, imul_ids);
            }
            Stmt::Let { init: Some(e), .. } => {
                rewrite_imul_calls_in_expr(e, imul_ids);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                rewrite_imul_calls_in_expr(condition, imul_ids);
                rewrite_imul_calls_in_stmts(then_branch, imul_ids);
                if let Some(eb) = else_branch {
                    rewrite_imul_calls_in_stmts(eb, imul_ids);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                rewrite_imul_calls_in_expr(condition, imul_ids);
                rewrite_imul_calls_in_stmts(body, imul_ids);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    rewrite_imul_calls_in_stmts(std::slice::from_mut(init_stmt), imul_ids);
                }
                if let Some(c) = condition {
                    rewrite_imul_calls_in_expr(c, imul_ids);
                }
                if let Some(u) = update {
                    rewrite_imul_calls_in_expr(u, imul_ids);
                }
                rewrite_imul_calls_in_stmts(body, imul_ids);
            }
            _ => {}
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
        Expr::IndexGet { object, index } => {
            rewrite_imul_calls_in_expr(object, imul_ids);
            rewrite_imul_calls_in_expr(index, imul_ids);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            rewrite_imul_calls_in_expr(object, imul_ids);
            rewrite_imul_calls_in_expr(index, imul_ids);
            rewrite_imul_calls_in_expr(value, imul_ids);
        }
        Expr::Array(elems) => {
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
