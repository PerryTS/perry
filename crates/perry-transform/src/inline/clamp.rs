use perry_hir::{Expr, Function, Stmt};

pub fn is_clamp3(f: &Function) -> bool {
    if f.is_async || f.is_generator || f.params.len() != 3 || f.body.len() != 3 {
        return false;
    }
    let (v_id, lo_id, hi_id) = (f.params[0].id, f.params[1].id, f.params[2].id);
    let Stmt::If {
        condition:
            Expr::Compare {
                op: perry_hir::CompareOp::Lt,
                left,
                right,
            },
        then_branch,
        else_branch: None,
    } = &f.body[0]
    else {
        return false;
    };
    if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == v_id) {
        return false;
    }
    if !matches!(right.as_ref(), Expr::LocalGet(id) if *id == lo_id) {
        return false;
    }
    if then_branch.len() != 1
        || !matches!(&then_branch[0], Stmt::Return(Some(Expr::LocalGet(id))) if *id == lo_id)
    {
        return false;
    }
    let Stmt::If {
        condition:
            Expr::Compare {
                op: perry_hir::CompareOp::Gt,
                left,
                right,
            },
        then_branch,
        else_branch: None,
    } = &f.body[1]
    else {
        return false;
    };
    if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == v_id) {
        return false;
    }
    if !matches!(right.as_ref(), Expr::LocalGet(id) if *id == hi_id) {
        return false;
    }
    if then_branch.len() != 1
        || !matches!(&then_branch[0], Stmt::Return(Some(Expr::LocalGet(id))) if *id == hi_id)
    {
        return false;
    }
    matches!(&f.body[2], Stmt::Return(Some(Expr::LocalGet(id))) if *id == v_id)
}

/// `function clampU8(v) { if (v < 0) return 0; if (v > 255) return 255; return v|0; }`
pub fn is_clamp_u8(f: &Function) -> bool {
    if f.is_async || f.is_generator || f.params.len() != 1 || f.body.len() != 3 {
        return false;
    }
    let v_id = f.params[0].id;
    let Stmt::If {
        condition:
            Expr::Compare {
                op: perry_hir::CompareOp::Lt,
                left,
                right,
            },
        then_branch,
        else_branch: None,
    } = &f.body[0]
    else {
        return false;
    };
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
    let Stmt::If {
        condition:
            Expr::Compare {
                op: perry_hir::CompareOp::Gt,
                left,
                right,
            },
        then_branch,
        else_branch: None,
    } = &f.body[1]
    else {
        return false;
    };
    if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == v_id) {
        return false;
    }
    if !matches!(right.as_ref(), Expr::Integer(255)) {
        return false;
    }
    matches!(
        then_branch.as_slice(),
        [Stmt::Return(Some(Expr::Integer(255)))]
    )
}

// ── Math.imul polyfill detection ──────────────────────────────────────────
