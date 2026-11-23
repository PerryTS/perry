//! Rule RP1: the positional steps of an array pattern read fixed indices.
//!
//! An array binding pattern over a guarded iterator record
//! (`perry_hir::iterator_record::IteratorRecordPlan`) declares its cursor
//! `let c = arrayRecordIndex(0)` and pulls each element with one step,
//! guarded by the done local `d` of the pattern (`pattern_binding.rs`):
//!
//! ```text
//! if (d == true) { v = undefined } else {
//!     [d = true;]
//!     let r = use_iter == true ? iteratorStep(..)
//!           : c < arrayRecordLength(p) ? (v = p[arrayRecordIndex(c)], c++, false)
//!           : (v = undefined, true);
//!     if (r == true) { d = true } else { [d = false] }
//! }
//! ```
//!
//! In the array arm step j runs only after steps 0 .. j-1 each ran and
//! advanced `c` once (a done step sets `d`, and `d` is never set back to
//! anything but `true` outside a step that advanced), so it reads index
//! j: its `arrayRecordIndex(c)` is the Number j (`Gen::num_literal`), and
//! the read of an Array that has that index slot reads the slot alone
//! (`Model::exact_index_slot`). The scan stops at the first other use of
//! `c` (a rest loop, the close, a for-of update), at a closure, and at a
//! write of `d` that is not `true`, so every numbered step is one of the
//! leading positional steps of its pattern.
use perry_hir::{CompareOp, Expr, Stmt};

type Local = perry_hir::types::LocalId;

/// The cursor `c` of `let c = arrayRecordIndex(0)`.
pub(crate) fn cursor_let(s: &Stmt) -> Option<Local> {
    let Stmt::Let {
        id,
        init:
            Some(Expr::NativeMethodCall {
                module,
                method,
                args,
                ..
            }),
        ..
    } = s
    else {
        return None;
    };
    (module == "__perry_runtime"
        && method == "arrayRecordIndex"
        && matches!(args.as_slice(), [Expr::Integer(0)]))
    .then_some(*id)
}

/// The positional steps of cursor `c` in `rest` (the statements after its
/// `let`), each the address of its `arrayRecordIndex(c)` arguments with its
/// position.
pub(crate) fn positional_steps(c: Local, rest: &[Stmt], out: &mut Vec<(usize, u32)>) {
    let mut done: Option<Local> = None;
    let mut j = 0u32;
    for s in rest {
        if let Some((d, at)) = step_if(c, s) {
            if done.is_some_and(|x| x != d) {
                return;
            }
            done = Some(d);
            out.push((at, j));
            j += 1;
            continue;
        }
        if stmt_bad(s, c, done) {
            return;
        }
    }
}

/// `if (d == true) { .. } else { [d = true;] let r = STEP; if (r == true)
/// { d = true } else { [d = false] } }` with STEP the one step of `c`:
/// (d, the address of the `arrayRecordIndex(c)` arguments of the step).
fn step_if(c: Local, s: &Stmt) -> Option<(Local, usize)> {
    let Stmt::If {
        condition,
        then_branch,
        else_branch: Some(eb),
    } = s
    else {
        return None;
    };
    let d = is_true_test(condition)?;
    if then_branch.iter().any(|s| stmt_bad(s, c, None)) {
        return None;
    }
    let mut it = eb.iter();
    let mut s = it.next()?;
    if is_set_true(s, d) {
        s = it.next()?;
    }
    let Stmt::Let {
        id: r,
        init: Some(step),
        ..
    } = s
    else {
        return None;
    };
    let at = step_of(c, step)?;
    let Stmt::If {
        condition,
        then_branch,
        else_branch,
    } = it.next()?
    else {
        return None;
    };
    if is_true_test(condition) != Some(*r)
        || !matches!(then_branch.as_slice(), [x] if is_set_true(x, d))
    {
        return None;
    }
    let tail_ok = match else_branch.as_deref() {
        None | Some([]) => true,
        Some([Stmt::Expr(Expr::LocalSet(x, v))]) => {
            *x == d && matches!(v.as_ref(), Expr::Bool(false))
        }
        _ => false,
    };
    if !tail_ok || it.next().is_some() {
        return None;
    }
    Some((d, at))
}

/// `use_iter == true ? <no c> : c < len ? (v = p[arrayRecordIndex(c)] | p[..], c++, false)
/// : (v = undefined, true)`.
fn step_of(c: Local, e: &Expr) -> Option<usize> {
    let Expr::Conditional {
        then_expr,
        else_expr,
        ..
    } = e
    else {
        return None;
    };
    if expr_uses(then_expr, c) {
        return None;
    }
    let Expr::Conditional {
        condition,
        then_expr,
        else_expr,
    } = else_expr.as_ref()
    else {
        return None;
    };
    let Expr::Compare {
        op: CompareOp::Lt,
        left,
        right, ..
    } = condition.as_ref()
    else {
        return None;
    };
    if !matches!(left.as_ref(), Expr::LocalGet(x) if *x == c)
        || expr_uses(right, c)
        || expr_uses(else_expr, c)
    {
        return None;
    }
    let Expr::Sequence(seq) = then_expr.as_ref() else {
        return None;
    };
    let [read, Expr::Update { id, .. }, Expr::Bool(false)] = seq.as_slice() else {
        return None;
    };
    if *id != c {
        return None;
    }
    let read = match read {
        Expr::LocalSet(_, v) => v.as_ref(),
        x => x,
    };
    let Expr::IndexGet { object, index, .. } = read else {
        return None;
    };
    let Expr::NativeMethodCall {
        module,
        method,
        args,
        ..
    } = index.as_ref()
    else {
        return None;
    };
    (module == "__perry_runtime"
        && method == "arrayRecordIndex"
        && matches!(args.as_slice(), [Expr::LocalGet(x)] if *x == c)
        && !expr_uses(object, c))
    .then_some(args.as_ptr() as usize)
}

/// `x == true`: x.
fn is_true_test(e: &Expr) -> Option<Local> {
    match e {
        Expr::Compare {
            op: CompareOp::Eq,
            left,
            right, ..
        } => match (left.as_ref(), right.as_ref()) {
            (Expr::LocalGet(x), Expr::Bool(true)) => Some(*x),
            _ => None,
        },
        _ => None,
    }
}

fn is_set_true(s: &Stmt, d: Local) -> bool {
    matches!(s, Stmt::Expr(Expr::LocalSet(x, v)) if *x == d && matches!(v.as_ref(), Expr::Bool(true)))
}

/// Does `s` end the scan: it uses `c`, holds a closure, or writes `d` with
/// anything but `true`?
fn stmt_bad(s: &Stmt, c: Local, d: Option<Local>) -> bool {
    if let Stmt::Let { id, .. } = s {
        if *id == c || Some(*id) == d {
            return true;
        }
    }
    perry_hir::walker::stmt_any_expr(s, &mut |e| expr_bad(e, c, d))
}

fn expr_bad(e: &Expr, c: Local, d: Option<Local>) -> bool {
    let here = match e {
        Expr::LocalGet(x) | Expr::Update { id: x, .. } => *x == c,
        Expr::LocalSet(x, v) => {
            *x == c || (Some(*x) == d && !matches!(v.as_ref(), Expr::Bool(true)))
        }
        Expr::Closure { .. } => true,
        _ => false,
    } || matches!(e, Expr::Update { id, .. } if Some(*id) == d);
    if here {
        return true;
    }
    let mut bad = false;
    perry_hir::walker::walk_expr_children(e, &mut |k| bad = bad || expr_bad(k, c, d));
    bad
}

fn expr_uses(e: &Expr, c: Local) -> bool {
    expr_bad(e, c, None)
}
