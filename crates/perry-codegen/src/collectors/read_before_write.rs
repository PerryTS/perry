//! The one read-before-write predicate of the per-body "typed by
//! construction" facts (#12658).
//!
//! A hoisted `var` (and a `let x;`) lowers to a `Let` with an `undefined`
//! seed ahead of its real writes. A fact that proves a local's kind from its
//! real writes alone and skips that seed (`typed_locals`, and the optimistic
//! `undefined` seed `integer_locals` exempts) holds only when no read of the
//! local can run before a real write: such a read sees `undefined`, which is
//! neither a String, a Number nor an int32. Every such fact asks this walk;
//! none keeps its own copy.
//!
//! The walk is a conservative definite-assignment pass over the body. A read
//! is safe only when a write to the local dominates it structurally; a write
//! inside a conditional part (`&&`, `||`, `??`, `?:`, one `if` arm, a loop
//! body, a `try` block, a `switch` case) does not count after it. A closure
//! may run any time after its creation, so creating it counts as a read of
//! every local it captures or reads in its body.
//!
//! Whether a local is flagged depends only on the body and that local, never
//! on which other locals are candidates, so a caller may exclude the result
//! from its candidates up front and run its fixed point once.

use std::collections::HashSet;

use perry_hir::{Expr, Stmt};

/// Locals some read of which may run before any write (so it may see the
/// `undefined` a `var` starts as). Conservative: a read is safe only when a
/// write to the local dominates it structurally.
pub(crate) fn reads_before_write(stmts: &[Stmt], candidates: &HashSet<u32>) -> HashSet<u32> {
    let mut bad = HashSet::new();
    let mut da = HashSet::new();
    da_stmts(stmts, candidates, &mut da, &mut bad);
    bad
}

fn da_stmts(stmts: &[Stmt], c: &HashSet<u32>, da: &mut HashSet<u32>, bad: &mut HashSet<u32>) {
    for s in stmts {
        da_stmt(s, c, da, bad);
    }
}

fn da_stmt(s: &Stmt, c: &HashSet<u32>, da: &mut HashSet<u32>, bad: &mut HashSet<u32>) {
    match s {
        Stmt::Let { id, init, .. } => match init {
            // The `undefined` seed of a hoisted `var` is not an assignment.
            None | Some(Expr::Undefined) => {
                da.remove(id);
            }
            Some(e) => {
                da_expr(e, c, da, bad);
                da.insert(*id);
            }
        },
        Stmt::Expr(e) | Stmt::Throw(e) => da_expr(e, c, da, bad),
        Stmt::Return(e) => {
            if let Some(e) = e {
                da_expr(e, c, da, bad);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            da_expr(condition, c, da, bad);
            let mut t = da.clone();
            da_stmts(then_branch, c, &mut t, bad);
            let mut f = da.clone();
            if let Some(b) = else_branch {
                da_stmts(b, c, &mut f, bad);
            }
            *da = t.intersection(&f).copied().collect();
        }
        Stmt::While { condition, body } => {
            da_expr(condition, c, da, bad);
            let mut b = da.clone();
            da_stmts(body, c, &mut b, bad);
        }
        Stmt::DoWhile { body, condition } => {
            // The body runs once before the condition, but a `continue`
            // can reach the condition from anywhere in it.
            let mut b = da.clone();
            da_stmts(body, c, &mut b, bad);
            let mut cnd = da.clone();
            da_expr(condition, c, &mut cnd, bad);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                da_stmt(i, c, da, bad);
            }
            if let Some(cnd) = condition {
                da_expr(cnd, c, da, bad);
            }
            let mut b = da.clone();
            da_stmts(body, c, &mut b, bad);
            // `continue` reaches the update from anywhere in the body.
            if let Some(u) = update {
                let mut u_da = da.clone();
                da_expr(u, c, &mut u_da, bad);
            }
        }
        Stmt::Labeled { body, .. } => {
            let mut b = da.clone();
            da_stmt(body, c, &mut b, bad);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            let mut b = da.clone();
            da_stmts(body, c, &mut b, bad);
            if let Some(cc) = catch {
                let mut k = da.clone();
                da_stmts(&cc.body, c, &mut k, bad);
            }
            if let Some(f) = finally {
                let mut k = da.clone();
                da_stmts(f, c, &mut k, bad);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            da_expr(discriminant, c, da, bad);
            for case in cases {
                let mut k = da.clone();
                if let Some(t) = &case.test {
                    da_expr(t, c, &mut k, bad);
                }
                da_stmts(&case.body, c, &mut k, bad);
            }
        }
        _ => {}
    }
}

/// Reads in evaluation order; a write inside a conditional part
/// (`&&`, `||`, `??`, `?:`) does not count after it.
fn da_expr(e: &Expr, c: &HashSet<u32>, da: &mut HashSet<u32>, bad: &mut HashSet<u32>) {
    match e {
        Expr::LocalGet(id) => {
            if c.contains(id) && !da.contains(id) {
                bad.insert(*id);
            }
        }
        Expr::Update { id, .. } => {
            if c.contains(id) && !da.contains(id) {
                bad.insert(*id);
            }
        }
        Expr::LocalSet(id, v) => {
            da_expr(v, c, da, bad);
            da.insert(*id);
        }
        Expr::Logical { left, right, .. } => {
            da_expr(left, c, da, bad);
            let mut r = da.clone();
            da_expr(right, c, &mut r, bad);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            da_expr(condition, c, da, bad);
            let mut t = da.clone();
            da_expr(then_expr, c, &mut t, bad);
            let mut f = da.clone();
            da_expr(else_expr, c, &mut f, bad);
            *da = t.intersection(&f).copied().collect();
        }
        Expr::Closure {
            captures,
            mutable_captures,
            body,
            ..
        } => {
            // Creating the closure reads what it captures (by value unless
            // boxed, and boxed locals are never candidates).
            let mut read = |id: &u32| {
                if c.contains(id) && !da.contains(id) {
                    bad.insert(*id);
                }
            };
            captures
                .iter()
                .chain(mutable_captures.iter())
                .for_each(&mut read);
            fn gets(stmts: &[Stmt], f: &mut dyn FnMut(&u32)) {
                fn e(x: &Expr, f: &mut dyn FnMut(&u32)) {
                    match x {
                        Expr::LocalGet(id) | Expr::Update { id, .. } => f(id),
                        Expr::Closure { body, .. } => gets(body, f),
                        _ => {}
                    }
                    perry_hir::walker::walk_expr_children(x, &mut |y| e(y, f));
                }
                for s in stmts {
                    perry_hir::walker::stmt_any_expr(s, &mut |x| {
                        e(x, f);
                        false
                    });
                }
            }
            gets(body, &mut read);
        }
        _ => perry_hir::walker::walk_expr_children(e, &mut |x| da_expr(x, c, da, bad)),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use perry_hir::types::Type as HirType;
    use perry_hir::{BinaryOp, Expr, Stmt};

    use super::reads_before_write;

    const K: u32 = 1;
    const R: u32 = 2;
    const N: u32 = 3;
    const X: u32 = 4;

    fn let_(id: u32, init: Option<Expr>) -> Stmt {
        Stmt::Let {
            id,
            name: format!("v{id}"),
            ty: HirType::Any,
            mutable: true,
            init,
        }
    }

    fn set(id: u32, v: Expr) -> Stmt {
        Stmt::Expr(Expr::LocalSet(id, Box::new(v)))
    }

    fn get(id: u32) -> Expr {
        Expr::LocalGet(id)
    }

    fn bin(op: BinaryOp, l: Expr, r: Expr) -> Expr {
        Expr::Binary {
            op,
            left: Box::new(l),
            right: Box::new(r),
        }
    }

    /// #12658: `var r = 0; if (k) { r = n + 1; } var n = 5; n = n * 2;
    /// return r + n;` with `var n` hoisted as its `undefined` seed.
    fn issue_body() -> Vec<Stmt> {
        vec![
            let_(R, Some(Expr::Integer(0))),
            let_(N, Some(Expr::Undefined)),
            Stmt::If {
                condition: get(K),
                then_branch: vec![set(R, bin(BinaryOp::Add, get(N), Expr::Integer(1)))],
                else_branch: None,
            },
            let_(N, Some(Expr::Integer(5))),
            set(N, bin(BinaryOp::Mul, get(N), Expr::Integer(2))),
            Stmt::Return(Some(bin(BinaryOp::Add, get(R), get(N)))),
        ]
    }

    /// The control: the same seed, every read after a real write.
    fn control_body() -> Vec<Stmt> {
        vec![
            let_(X, Some(Expr::Undefined)),
            set(X, Expr::Integer(4)),
            set(X, Expr::Integer(6)),
            Stmt::Return(Some(get(X))),
        ]
    }

    fn integer_locals(body: &[Stmt]) -> HashSet<u32> {
        crate::collectors::integer_locals::collect_integer_locals(
            body,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        )
    }

    #[test]
    fn a_read_in_a_branch_above_the_declaration_sees_the_seed() {
        let all = HashSet::from([R, N]);
        assert_eq!(reads_before_write(&issue_body(), &all), HashSet::from([N]));
        assert!(reads_before_write(&control_body(), &HashSet::from([X])).is_empty());
    }

    #[test]
    fn an_int32_local_read_before_its_first_write_is_not_an_int32_local() {
        let ints = integer_locals(&issue_body());
        assert!(!ints.contains(&N), "{ints:?}");
        // `r = n + 1` relied on `n`, so it goes too.
        assert!(!ints.contains(&R), "{ints:?}");
        let ints = integer_locals(&control_body());
        assert!(ints.contains(&X), "{ints:?}");
    }

    #[test]
    fn the_shared_predicate_is_what_drops_the_seeded_int32_local() {
        // The same body with the early read moved below the declaration is
        // proven: only the read's position differs.
        let mut late = issue_body();
        let early = late.remove(2);
        late.insert(4, early);
        let ints = integer_locals(&late);
        assert!(ints.contains(&N), "{ints:?}");
    }
}
