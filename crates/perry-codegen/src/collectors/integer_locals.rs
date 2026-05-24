use perry_hir::{BinaryOp, Expr, Function, Stmt};
use std::collections::HashSet;

use super::*;

pub fn collect_integer_locals(
    stmts: &[perry_hir::Stmt],
    flat_const_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
) -> HashSet<u32> {
    let mut candidates: HashSet<u32> = HashSet::new();

    // Issue #50 bridge: pre-compute which locals are row-aliases of
    // flat-const 2D int arrays BEFORE collecting integer let ids, since
    // `collect_integer_let_ids` needs to recognize `let k = krow[j]`
    // (where krow is a flat-const row alias) as an int-producing init.
    let mut flat_row_alias_ids: HashSet<u32> = HashSet::new();
    collect_flat_row_aliases(stmts, flat_const_ids, &mut flat_row_alias_ids);

    collect_integer_let_ids(
        stmts,
        &mut candidates,
        flat_const_ids,
        &flat_row_alias_ids,
        clamp_fn_ids,
    );

    // Forward closure pass: extend the seed set with Lets whose init is
    // `is_int32_producing_expr` against the current candidate set.
    // The initial `collect_integer_let_ids` only seeds on syntactic
    // patterns (Integer literals, `(expr) | 0`, clamp calls, …) but
    // misses transitive int-stable Lets like `const hi = W - 1` where
    // `W` is itself a candidate. Iterate to a fixed point so chains
    // such as `const W = 3840` → `const hi = W - 1` → uses-of-hi
    // propagate cleanly.
    //
    // image_convolution's clampIdx-inlined `xx`/`yy` rely on this:
    // their write-set includes `LocalSet(xx, LocalGet(hi))`, and
    // without `hi` in the int-stable set the disqualifier marks the
    // assignment as non-int-producing and removes `xx`/`yy` from the
    // set — taking down the i32 shadow on every downstream use of
    // `idx = (row + xx) * 3` and forcing the inner kernel's address
    // generation back into double.
    loop {
        let before = candidates.len();
        collect_extra_integer_let_ids(
            stmts,
            &mut candidates,
            flat_const_ids,
            &flat_row_alias_ids,
            clamp_fn_ids,
        );
        if candidates.len() == before {
            break;
        }
    }

    // Iterate to a fixed point (issue #49): `is_int32_producing_expr` now
    // recognizes `LocalGet(id)` as int-producing when `id` is itself
    // int-stable, and `Add/Sub/Mul` as int-producing when both operands
    // are. That makes the analysis mutually recursive across locals —
    // disqualifying one candidate may cascade to other candidates whose
    // rhs referenced the first via LocalGet. Iterate until the set
    // stabilizes.
    loop {
        let mut disqualified: HashSet<u32> = HashSet::new();
        collect_non_int_localset_ids_in_stmts(
            stmts,
            &mut disqualified,
            &candidates,
            flat_const_ids,
            &flat_row_alias_ids,
            clamp_fn_ids,
        );
        let before = candidates.len();
        candidates.retain(|id| !disqualified.contains(id));
        if candidates.len() == before {
            break;
        }
    }
    candidates
}

/// Walk all `Stmt::Let { id, init: Some(e), .. }` and add `id` to
/// `out` when `e` is `is_int32_producing_expr` against the *current*
/// `out` set. Used by `collect_integer_locals` to take the
/// syntactic seed set's transitive closure: e.g. `const W = 3840` is
/// seeded on the initial pass, then `const hi = W - 1` lands here on
/// the second pass because `W` is already in the set, and any Let
/// whose init reduces to `is_int32_producing_expr` over `hi` lands
/// on the third pass.
pub fn collect_extra_integer_let_ids(
    stmts: &[perry_hir::Stmt],
    out: &mut HashSet<u32>,
    flat_const_ids: &HashSet<u32>,
    flat_row_alias_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
) {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Let {
                id,
                init: Some(init),
                ..
            } => {
                // Same `>>> 0` exclusion as the syntactic seed in
                // `collect_integer_let_ids`: u32 values can't round-trip
                // through an i32 slot.
                if !is_ushr_zero(init)
                    && !out.contains(id)
                    && is_int32_producing_expr(
                        init,
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    )
                {
                    out.insert(*id);
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_extra_integer_let_ids(
                    then_branch,
                    out,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
                if let Some(eb) = else_branch {
                    collect_extra_integer_let_ids(
                        eb,
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_extra_integer_let_ids(
                        std::slice::from_ref(init_stmt),
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
                collect_extra_integer_let_ids(
                    body,
                    out,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_extra_integer_let_ids(
                    body,
                    out,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_extra_integer_let_ids(
                    body,
                    out,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
                if let Some(c) = catch {
                    collect_extra_integer_let_ids(
                        &c.body,
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
                if let Some(f) = finally {
                    collect_extra_integer_let_ids(
                        f,
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    collect_extra_integer_let_ids(
                        &c.body,
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
            }
            Stmt::Labeled { body, .. } => {
                collect_extra_integer_let_ids(
                    std::slice::from_ref(body.as_ref()),
                    out,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
            }
            _ => {}
        }
    }
}

pub fn collect_flat_row_aliases(
    stmts: &[perry_hir::Stmt],
    flat_const_ids: &HashSet<u32>,
    out: &mut HashSet<u32>,
) {
    use perry_hir::{Expr, Stmt};
    for s in stmts {
        match s {
            Stmt::Let {
                id,
                init: Some(Expr::IndexGet { object, .. }),
                mutable: false,
                ..
            } => {
                if let Expr::LocalGet(const_id) = object.as_ref() {
                    if flat_const_ids.contains(const_id) {
                        out.insert(*id);
                    }
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_flat_row_aliases(then_branch, flat_const_ids, out);
                if let Some(eb) = else_branch {
                    collect_flat_row_aliases(eb, flat_const_ids, out);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_flat_row_aliases(std::slice::from_ref(init_stmt), flat_const_ids, out);
                }
                collect_flat_row_aliases(body, flat_const_ids, out);
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_flat_row_aliases(body, flat_const_ids, out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_flat_row_aliases(body, flat_const_ids, out);
                if let Some(catch) = catch {
                    collect_flat_row_aliases(&catch.body, flat_const_ids, out);
                }
                if let Some(finally) = finally {
                    collect_flat_row_aliases(finally, flat_const_ids, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_flat_row_aliases(&case.body, flat_const_ids, out);
                }
            }
            Stmt::Labeled { body, .. } => {
                collect_flat_row_aliases(std::slice::from_ref(body.as_ref()), flat_const_ids, out);
            }
            _ => {}
        }
    }
}

/// Returns `true` if evaluating `e` yields a value that will already be
/// integer-valued — so writing it into a local's i32 slot is lossless.
///
/// Accepted shapes:
///   - `Expr::Integer(_)`: trivially integer.
///   - `(expr) | 0` and `(expr) >>> 0`: the JS ToInt32 / ToUint32 idiom —
///     always yields a 32-bit integer regardless of the inner expression.
///   - Pure bitwise ops (`&`, `|`, `^`, `<<`, `>>`, `>>>`): per JS spec
///     these coerce both operands to int32 and return int32.
///   - `Expr::Update`: `++` / `--` on an integer-stable local (we don't
///     verify transitively; if the target isn't qualified, the whole chain
///     collapses anyway).
///   - (issue #49) `LocalGet(id)` when `id` is itself in `known_int_locals` —
///     enables the accumulator pattern `acc = acc + int_expr` without
///     requiring a `| 0` wrapper on every write.
///   - (issue #49) `Uint8ArrayGet` / `BufferIndexGet`: typed-array byte
///     reads return u8 values; always fit in i32.
///   - (issue #49) `Add` / `Sub` / `Mul` when both operands are
///     int-producing. The sum/product may overflow i32, but the existing
///     i32-slot machinery already accepts this risk — the double slot is
///     maintained in parallel and reads past i32::MAX were already wrong
///     for `| 0`-written accumulators.
///
/// Rejected: everything else (notably `Div`/`Mod` without a `|0` wrapper,
/// bare floats, calls returning doubles, etc.) because they can produce
/// non-integer doubles at runtime.
pub fn is_int32_producing_expr(
    e: &perry_hir::Expr,
    known_int_locals: &HashSet<u32>,
    flat_const_ids: &HashSet<u32>,
    flat_row_alias_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
) -> bool {
    use perry_hir::{BinaryOp, Expr};
    match e {
        Expr::Integer(_) => true,
        Expr::Update { .. } => true,
        Expr::Binary { op, right, .. }
            if matches!(op, BinaryOp::BitOr | BinaryOp::UShr)
                && matches!(right.as_ref(), Expr::Integer(0)) =>
        {
            true
        }
        Expr::Binary { op, left, right }
            if matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul) =>
        {
            is_int32_producing_expr(
                left,
                known_int_locals,
                flat_const_ids,
                flat_row_alias_ids,
                clamp_fn_ids,
            ) && is_int32_producing_expr(
                right,
                known_int_locals,
                flat_const_ids,
                flat_row_alias_ids,
                clamp_fn_ids,
            )
        }
        Expr::Call { callee, .. } => {
            if let Expr::FuncRef(fid) = callee.as_ref() {
                clamp_fn_ids.contains(fid)
            } else {
                false
            }
        }
        Expr::Binary { op, .. } => matches!(
            op,
            BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr
                | BinaryOp::UShr
        ),
        Expr::LocalGet(id) => known_int_locals.contains(id),
        Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. } => true,
        Expr::MathImul(_, _) => true, // Math.imul always returns i32
        // Issue #50 bridge: element access on a flat-const 2D int array
        // produces i32. Two shapes:
        //   - inline `X[i][j]`: IndexGet(IndexGet(LocalGet(X), i), j)
        //   - aliased `krow[j]`: IndexGet(LocalGet(alias), j)
        Expr::IndexGet { object, .. } => match object.as_ref() {
            Expr::IndexGet { object: inner, .. } => {
                matches!(inner.as_ref(), Expr::LocalGet(id) if flat_const_ids.contains(id))
            }
            Expr::LocalGet(id) => flat_row_alias_ids.contains(id),
            _ => false,
        },
        _ => false,
    }
}
