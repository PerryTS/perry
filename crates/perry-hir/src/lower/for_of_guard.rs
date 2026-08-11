//! #7760 item 1: the runtime guard that lets a `for…of` over a proven array
//! honour a replaced `Array.prototype[Symbol.iterator]`.
//!
//! Split out of `stmt_loops.rs` to keep that file under the 2000-line CI cap.

use anyhow::Result;
use swc_ecma_ast as ast;

use super::stmt_loops::lower_stmt_for_of_inner;
use super::LoweringContext;
use crate::ir::{Expr, Stmt};
use crate::Module;

/// #7760 item 1: `for…of` over a statically-proven array desugars to an index
/// loop that never consults the iteration protocol, so a patched
/// `Array.prototype[Symbol.iterator]` was ignored there even after #7542 fixed
/// every spread form.
///
/// The patch is a RUNTIME fact and the index/lazy choice is a COMPILE-TIME one,
/// so the only correct answer is to emit both and branch. Three properties made
/// the shape non-obvious:
///
///   * It must stay LAZY. Materializing the protocol once at loop entry — what
///     spread now does — is eager where node is lazy: an early `break` would
///     over-pull, a side-effecting iterator would run extra steps, and an
///     infinite patched iterator would hang instead of terminating.
///   * The index loop must be emitted BYTE-IDENTICALLY. Threading the element
///     through a shared `__item` temp (one loop, loop-invariant branch) would
///     have cost nothing in LLVM but would have broken the HIR-level pattern
///     matchers behind the element-shape clone (#7612), the dense spread path
///     (#7533) and the packed-f64 / i32-counter loop specializations — a silent
///     regression on the hottest loop in the language.
///   * The check belongs at loop ENTRY, not per iteration — which is also what
///     the spec says: `for…of` performs GetIterator exactly once, so a patch
///     landing mid-loop must not change the iterator already in hand.
///
/// So the fast arm pays one volatile `i8` load and a predictable branch per
/// LOOP, and the loop inside it is the same `Stmt::For` as before.
pub(crate) fn lower_stmt_for_of(
    ctx: &mut LoweringContext,
    module: &mut Module,
    for_of_stmt: &ast::ForOfStmt,
) -> Result<()> {
    // Lower the index form first and see whether it is a proven-array loop that
    // needs the guard (the callee reports it). If so, lower the lazy form too
    // and splice the two into an `if`. Lowering twice re-runs the setup rather
    // than duplicating it here, so the two arms cannot drift apart.
    let mark = module.init.len();
    let guarded = lower_stmt_for_of_inner(ctx, module, for_of_stmt, None)?;
    if !guarded {
        return Ok(());
    }
    let index_arm: Vec<Stmt> = module.init.split_off(mark);
    lower_stmt_for_of_inner(ctx, module, for_of_stmt, Some(true))?;
    let lazy_arm: Vec<Stmt> = module.init.split_off(mark);
    module.init.push(Stmt::If {
        condition: Expr::ArrayIterationPatched,
        then_branch: lazy_arm,
        else_branch: Some(index_arm),
    });
    Ok(())
}
