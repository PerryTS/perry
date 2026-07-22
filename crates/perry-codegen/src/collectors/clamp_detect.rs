use perry_hir::{BinaryOp, Expr, Function, Stmt};

pub fn detect_clamp3(f: &Function) -> Option<(u32, u32, u32)> {
    if f.is_async || f.is_generator || f.params.len() != 3 {
        return None;
    }
    if !matches!(f.return_type, perry_hir::types::Type::Number) {
        return None;
    }
    if f.body.len() != 3 {
        return None;
    }
    let (v_id, lo_id, hi_id) = (f.params[0].id, f.params[1].id, f.params[2].id);
    // [0] If { cond: Compare(Lt, v, lo), then: [Return(lo)] }
    if let Stmt::If {
        condition:
            Expr::Compare {
                op: perry_hir::CompareOp::Lt,
                left,
                right,
            },
        then_branch,
        else_branch: None,
    } = &f.body[0]
    {
        if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == v_id) {
            return None;
        }
        if !matches!(right.as_ref(), Expr::LocalGet(id) if *id == lo_id) {
            return None;
        }
        if then_branch.len() != 1 {
            return None;
        }
        if !matches!(&then_branch[0], Stmt::Return(Some(Expr::LocalGet(id))) if *id == lo_id) {
            return None;
        }
    } else {
        return None;
    }
    // [1] If { cond: Compare(Gt, v, hi), then: [Return(hi)] }
    if let Stmt::If {
        condition:
            Expr::Compare {
                op: perry_hir::CompareOp::Gt,
                left,
                right,
            },
        then_branch,
        else_branch: None,
    } = &f.body[1]
    {
        if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == v_id) {
            return None;
        }
        if !matches!(right.as_ref(), Expr::LocalGet(id) if *id == hi_id) {
            return None;
        }
        if then_branch.len() != 1 {
            return None;
        }
        if !matches!(&then_branch[0], Stmt::Return(Some(Expr::LocalGet(id))) if *id == hi_id) {
            return None;
        }
    } else {
        return None;
    }
    // [2] Return(v)
    if !matches!(&f.body[2], Stmt::Return(Some(Expr::LocalGet(id))) if *id == v_id) {
        return None;
    }
    Some((v_id, lo_id, hi_id))
}

/// Detect a 1-param clampU8 pattern: `if (v < 0) return 0; if (v > 255) return 255; return v|0;`
///
/// The third statement must actually coerce (`v | 0`, another bitwise op, or
/// an integer literal). A body ending in bare `return v;` passes a fractional
/// in-range `v` through unchanged, so treating its callers' results as
/// int-producing (clamp_u8_functions feeds `clamp_fn_ids` as an
/// argument-INdependent admission in `collect_integer_locals`) would put a
/// truncating i32 shadow slot on a non-integer value.
pub fn detect_clamp_u8(f: &Function) -> bool {
    if f.is_async || f.is_generator || f.params.len() != 1 {
        return false;
    }
    if f.body.len() != 3 {
        return false;
    }
    let v_id = f.params[0].id;
    if let Stmt::If {
        condition:
            Expr::Compare {
                op: perry_hir::CompareOp::Lt,
                left,
                right,
            },
        then_branch,
        else_branch: None,
    } = &f.body[0]
    {
        if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == v_id) {
            return false;
        }
        if !matches!(right.as_ref(), Expr::Integer(0)) {
            return false;
        }
        if !matches!(
            then_branch.as_slice(),
            [Stmt::Return(Some(Expr::Integer(0)))]
        ) {
            return false;
        }
    } else {
        return false;
    }
    if let Stmt::If {
        condition:
            Expr::Compare {
                op: perry_hir::CompareOp::Gt,
                left,
                right,
            },
        then_branch,
        else_branch: None,
    } = &f.body[1]
    {
        if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == v_id) {
            return false;
        }
        if !matches!(right.as_ref(), Expr::Integer(255)) {
            return false;
        }
        if !matches!(
            then_branch.as_slice(),
            [Stmt::Return(Some(Expr::Integer(255)))]
        ) {
            return false;
        }
    } else {
        return false;
    }
    // [2] Return of an int-coercing expression (`v | 0`, bitwise, Integer).
    matches!(&f.body[2], Stmt::Return(Some(e)) if returns_int_expr(e))
}

/// A function is i64-specializable if it's a pure numeric recursive fn.
pub fn is_integer_specializable(f: &Function) -> bool {
    if f.is_async || f.is_generator {
        return false;
    }
    if !matches!(f.return_type, perry_hir::types::Type::Number) {
        return false;
    }
    if !f
        .params
        .iter()
        .all(|p| matches!(p.ty, perry_hir::types::Type::Number))
    {
        return false;
    }
    i64s_stmts(&f.body, f.id)
}
/// Detect functions that always return an integer value (all return paths
/// end with `| 0`, `>>> 0`, or another bitwise op). These functions can be
/// treated as int-producing at call sites, enabling the i32 fast path for
/// `h = userImul(h, p)` style patterns.
pub fn returns_integer(f: &Function) -> bool {
    if f.is_async || f.is_generator {
        return false;
    }
    if !matches!(f.return_type, perry_hir::types::Type::Number) {
        return false;
    }
    returns_int_stmts(&f.body)
}

pub fn returns_i32_identity_arg(f: &Function) -> bool {
    if f.is_async || f.is_generator || f.params.len() != 1 {
        return false;
    }
    if !matches!(f.return_type, perry_hir::types::Type::Number) {
        return false;
    }
    let param_id = f.params[0].id;
    matches!(
        f.body.as_slice(),
        [Stmt::Return(Some(expr))] if returns_i32_identity_expr(expr, param_id)
    )
}

fn returns_i32_identity_expr(expr: &Expr, param_id: u32) -> bool {
    match expr {
        Expr::LocalGet(id) => *id == param_id,
        Expr::Binary {
            op: BinaryOp::BitOr,
            left,
            right,
        } if matches!(right.as_ref(), Expr::Integer(0)) => {
            returns_i32_identity_expr(left, param_id)
        }
        _ => false,
    }
}

pub fn returns_int_stmts(ss: &[Stmt]) -> bool {
    for s in ss {
        match s {
            Stmt::Return(Some(e)) if !returns_int_expr(e) => {
                return false;
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                if !returns_int_stmts(then_branch) {
                    return false;
                }
                if let Some(eb) = else_branch {
                    if !returns_int_stmts(eb) {
                        return false;
                    }
                }
            }
            _ => {}
        }
    }
    true
}
pub fn returns_int_expr(e: &Expr) -> bool {
    match e {
        Expr::Integer(_) => true,
        // `>>> 0` produces u32, not a signed i32 — caller paths that
        // feed this through an i32 slot (`integer_locals` propagation,
        // i32-init fast path on Lets) sign-extend the high bit and
        // print the negative form via `toString(16)`. Exclude UShr
        // here so functions ending in `return h >>> 0` (FNV-1a hashes,
        // CRC, etc.) stay outside `returns_int_functions` and force
        // their callers onto the f64 result path.
        //
        // Other bitwise ops (BitAnd / BitOr / BitXor / Shl / Shr) all
        // produce SIGNED i32 per JS spec — safe to round-trip through
        // an i32 slot.
        Expr::Binary { op, .. } => matches!(
            op,
            BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr
        ),
        Expr::MathImul(_, _) => true,
        _ => false,
    }
}

pub fn i64s_stmts(ss: &[Stmt], sid: u32) -> bool {
    ss.iter().all(|s| match s {
        Stmt::Return(Some(e)) => i64s_expr(e, sid),
        Stmt::Return(None) => true,
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            i64s_expr(condition, sid)
                && i64s_stmts(then_branch, sid)
                && else_branch.as_ref().is_none_or(|eb| i64s_stmts(eb, sid))
        }
        Stmt::Expr(e) | Stmt::Let { init: Some(e), .. } => i64s_expr(e, sid),
        Stmt::Let { init: None, .. } => true,
        _ => false,
    })
}
pub fn i64s_expr(e: &Expr, sid: u32) -> bool {
    match e {
        Expr::Integer(_) | Expr::LocalGet(_) => true,
        // The i64 emitter lowers Number literals with `as i64`, so only admit
        // values that round-trip exactly — a fractional constant (`? 0.5 :`)
        // would silently truncate in the specialized body.
        Expr::Number(n) => *n as i64 as f64 == *n,
        Expr::Binary { op, left, right } => {
            matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul)
                && i64s_expr(left, sid)
                && i64s_expr(right, sid)
        }
        Expr::Compare { left, right, .. } => i64s_expr(left, sid) && i64s_expr(right, sid),
        Expr::Call { callee, args, .. } => {
            matches!(callee.as_ref(), Expr::FuncRef(id) if *id == sid)
                && args.iter().all(|a| i64s_expr(a, sid))
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => i64s_expr(condition, sid) && i64s_expr(then_expr, sid) && i64s_expr(else_expr, sid),
        _ => false,
    }
}
