//! SNAPK2b census: the `compare_literal` site kind (DESIGN27 §3), emitted in
//! instrumented compiles only (`PERRY_SNAP_CENSUS` set while compiling). An
//! ordinary compile emits nothing.
//!
//! An equality compare (`===`, `!==`, `==`, `!=`) calls
//! `js_snap_compare_literal(op, left, right, literal)` once both operands
//! are lowered, before the compare itself; `literal` has bit 0 (1) set when
//! the left (right) operand is a string literal. The runtime keeps a row only
//! when one operand is a host value or env string and the other a string:
//! the guard then holds the predicate, not the host value. A compare whose
//! operands a fast path lowers without `lower_expr` emits no hook, and the
//! read stays a value member. No table is emitted.

use std::cell::RefCell;

use perry_hir::{CompareOp, Expr};

use crate::types::{DOUBLE, I32};

use super::FnCtx;

struct Site {
    left: usize,
    right: usize,
    op: i32,
    literal: i32,
    values: [Option<String>; 2],
}

thread_local! {
    static SITES: RefCell<Vec<Site>> = const { RefCell::new(Vec::new()) };
}

/// `expr` is about to be lowered: note an equality compare's two operands.
pub(crate) fn note_parent(expr: &Expr) {
    if !super::snap_env_site::census_compile() {
        return;
    }
    let Expr::Compare { op, left, right } = expr else {
        return;
    };
    let op = match op {
        CompareOp::Eq => 0,
        CompareOp::Ne => 1,
        CompareOp::LooseEq => 2,
        CompareOp::LooseNe => 3,
        _ => return,
    };
    let literal = i32::from(matches!(**left, Expr::String(_)))
        | (i32::from(matches!(**right, Expr::String(_))) << 1);
    SITES.with(|s| {
        s.borrow_mut().push(Site {
            left: &**left as *const Expr as usize,
            right: &**right as *const Expr as usize,
            op,
            literal,
            values: [None, None],
        })
    });
}

/// `expr` was lowered to `value`: when it is an operand of a noted compare,
/// keep it; once both are kept, emit the hook.
pub(crate) fn lowered(ctx: &mut FnCtx<'_>, expr: &Expr, value: &str) {
    if !super::snap_env_site::census_compile() {
        return;
    }
    let addr = expr as *const Expr as usize;
    let ready = SITES.with(|s| {
        let mut s = s.borrow_mut();
        let i = s.iter().rposition(|t| {
            (t.left == addr && t.values[0].is_none()) || (t.right == addr && t.values[1].is_none())
        })?;
        let t = &mut s[i];
        let side = usize::from(!(t.left == addr && t.values[0].is_none()));
        t.values[side] = Some(value.to_owned());
        if t.values.iter().all(Option::is_some) {
            let t = s.swap_remove(i);
            let [l, r] = t.values;
            Some((t.op, l.unwrap(), r.unwrap(), t.literal))
        } else {
            None
        }
    });
    if let Some((op, l, r, literal)) = ready {
        ctx.block().call_void(
            "js_snap_compare_literal",
            &[
                (I32, &op.to_string()),
                (DOUBLE, &l),
                (DOUBLE, &r),
                (I32, &literal.to_string()),
            ],
        );
    }
}
