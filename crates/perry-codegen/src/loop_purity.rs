//! Loop body purity analysis for issue #74.
//!
//! Detects loop bodies that have no LLVM-visible observable side effect.
//! Such bodies trigger clang -O3's loop-deletion / IndVarSimplify passes
//! to fold the loop to its closed-form result, which means a tight
//! `for (let i=0; i<N; i++) {}` between two `Date.now()` calls
//! would report 0ms wall-clock — confusingly making `Date.now()` look
//! broken when in fact the loop never ran.
//!
//! When [`body_needs_asm_barrier`] returns true, `lower_for`
//! / `lower_while` / `lower_do_while` insert an empty `asm sideeffect`
//! barrier in the body. The barrier is opaque to the optimizer (it
//! cannot prove the asm has no effect) so the loop is preserved
//! end-to-end, and emits zero machine instructions.
//!
//! The whitelist is intentionally narrow: anything that could throw,
//! call, allocate, mutate the heap, or yield to async machinery is
//! treated as a side effect. This means real workloads (array writes,
//! method calls, property mutations) are unaffected — vectorization
//! and LICM still apply because we don't insert the barrier there.
//!
//! Issue #140: accumulator loops like `for (let i=0; i<N; i++) sum+=1;`
//! used to trip this analysis as "pure" (the body's only effect is a
//! `LocalSet` to an outer-scope local). With the barrier in place, LLVM's
//! loop vectorizer refuses to widen the fadd reduction into a `<2 x double>`
//! parallel-accumulator reduction even though the body is an otherwise-
//! trivial induction. But for those loops the barrier is superfluous —
//! `sum` is read after the loop (`console.log("sum:" + sum)`) so the
//! accumulator's final value is already observable without any asm
//! placeholder. [`body_needs_asm_barrier`] refuses the barrier when the
//! body writes to any outer-scope local (including the loop counter
//! itself when declared outside); that leaves truly-empty bodies
//! (`for (;;) {}`, `while (cond) {}`) — the #74 repro case — as the only
//! class that still receives the barrier.

use perry_hir::{CompareOp, Expr, Stmt, UnaryOp};
use std::collections::HashSet;

/// True when the body needs an `asm sideeffect` barrier inserted. This is
/// the stricter combination of "LLVM-pure" AND "no outer-scope write":
///   - Pure ensures the barrier is *legal* to add (we're not masking a
///     real side effect that would have kept the loop alive on its own).
///   - No outer-scope write ensures it's *needed* — if the body writes to
///     a local declared outside the loop, that local's observation after
///     the loop already prevents LLVM from folding the loop to a no-op.
pub(crate) fn body_needs_asm_barrier(body: &[Stmt]) -> bool {
    if !body.iter().all(stmt_is_pure) {
        return false;
    }
    let mut body_locals: HashSet<u32> = HashSet::new();
    collect_body_declared_locals(body, &mut body_locals);
    !body_writes_outside(body, &body_locals)
}

/// True when the loop body may allocate (or otherwise call into the runtime and
/// trip a GC), so a moving-GC back-edge poll (`js_gc_loop_safepoint`) must be
/// emitted to drain any deferred minor. Reuses the LLVM-purity whitelist: a body
/// that is fully LLVM-pure performs no call / allocation / heap mutation, so it
/// can never cross a nursery trigger and defer a collection — the poll would be
/// a guaranteed no-op that only defeats vectorization. Conservative in the SAFE
/// direction: anything not provably pure is treated as "may allocate" and gets
/// the poll. A spurious poll costs a little vectorization; a missing one only
/// delays a deferred minor to the next safepoint (bounded by the moving-GC hard
/// cap) — never a correctness or UAF hazard.
pub(crate) fn loop_may_allocate(body: &[Stmt], controls: &[&Expr]) -> bool {
    !body.iter().all(stmt_alloc_free) || controls.iter().any(|expr| !expr_alloc_free(expr))
}

/// Like `stmt_is_pure`, but the question is narrower — "can this allocate (or
/// call into the runtime and trip a GC)?" — so it additionally accepts element
/// ACCESS that never allocates: array/typed-array element READS and in-place
/// numeric updates never allocate, and typed-array element WRITES store into a
/// fixed-size backing buffer that never grows. Generic `IndexSet` is NOT
/// accepted: a plain JS-array index write can grow (reallocate) the backing
/// store. This lets a `for (…) acc += arr[i]` reduction stay poll-free (LLVM can
/// vectorize) while `keep.push({…})` (a Call) still gets its poll.
fn stmt_alloc_free(s: &Stmt) -> bool {
    match s {
        Stmt::Expr(e) => expr_alloc_free(e),
        Stmt::Let { init, .. } => init.as_ref().is_none_or(expr_alloc_free),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_alloc_free(condition)
                && then_branch.iter().all(stmt_alloc_free)
                && else_branch
                    .as_ref()
                    .is_none_or(|b| b.iter().all(stmt_alloc_free))
        }
        Stmt::While { condition, body } => {
            expr_alloc_free(condition) && body.iter().all(stmt_alloc_free)
        }
        Stmt::DoWhile { body, condition } => {
            expr_alloc_free(condition) && body.iter().all(stmt_alloc_free)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_deref().is_none_or(stmt_alloc_free)
                && condition.as_ref().is_none_or(expr_alloc_free)
                && update.as_ref().is_none_or(expr_alloc_free)
                && body.iter().all(stmt_alloc_free)
        }
        Stmt::Labeled { body, .. } => stmt_alloc_free(body),
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
        _ => false,
    }
}

fn expr_alloc_free(e: &Expr) -> bool {
    match e {
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::BigInt(_)
        | Expr::String(_)
        | Expr::This
        | Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::FuncRef(_)
        | Expr::ClassRef(_)
        | Expr::EnumMember { .. } => true,
        // Typed/buffer reads are fixed-layout numeric loads. Generic
        // IndexGet/IndexUpdate are deliberately excluded: proxies, accessors,
        // and coercion hooks can run user code and allocate.
        Expr::BufferIndexGet { buffer, index } => expr_alloc_free(buffer) && expr_alloc_free(index),
        Expr::Uint8ArrayGet { array, index } => expr_alloc_free(array) && expr_alloc_free(index),
        // Typed-array element WRITES store into a fixed-size backing buffer that
        // never grows/reallocates. (Generic `IndexSet` is deliberately absent —
        // a plain JS-array write can grow the array and allocate.)
        Expr::BufferIndexSet {
            buffer,
            index,
            value,
        } => expr_alloc_free(buffer) && expr_alloc_free(index) && expr_alloc_free(value),
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => expr_alloc_free(array) && expr_alloc_free(index) && expr_alloc_free(value),
        Expr::LocalSet(_, val) => expr_alloc_free(val),
        Expr::Compare {
            op: CompareOp::Eq | CompareOp::Ne,
            left,
            right,
        }
        | Expr::Logical { left, right, .. } => expr_alloc_free(left) && expr_alloc_free(right),
        Expr::Unary {
            op: UnaryOp::Not,
            operand,
        }
        | Expr::TypeOf(operand)
        | Expr::Void(operand) => expr_alloc_free(operand),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => expr_alloc_free(condition) && expr_alloc_free(then_expr) && expr_alloc_free(else_expr),
        _ => false,
    }
}

#[cfg(test)]
mod allocation_tests {
    use super::*;
    use perry_hir::BinaryOp;

    #[test]
    fn generic_index_update_keeps_the_loop_safepoint() {
        let update = Expr::IndexUpdate {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::Integer(0)),
            op: BinaryOp::Add,
            prefix: false,
        };
        assert!(loop_may_allocate(&[Stmt::Expr(update)], &[]));
    }

    #[test]
    fn allocating_loop_control_keeps_the_loop_safepoint() {
        let condition = Expr::IndexGet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::Integer(0)),
        };
        assert!(loop_may_allocate(&[], &[&condition]));
    }
}

fn stmt_is_pure(s: &Stmt) -> bool {
    match s {
        Stmt::Expr(e) => expr_is_pure(e),
        Stmt::Let { init, .. } => init.as_ref().is_none_or(expr_is_pure),
        Stmt::Return(_) | Stmt::Throw(_) => false,
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_is_pure(condition)
                && then_branch.iter().all(stmt_is_pure)
                && else_branch
                    .as_ref()
                    .is_none_or(|b| b.iter().all(stmt_is_pure))
        }
        // Nested loops: their own lowering applies the same analysis,
        // so reporting the outer body as pure when the inner is pure
        // is consistent (the inner loop will also get its barrier).
        Stmt::While { condition, body } => expr_is_pure(condition) && body.iter().all(stmt_is_pure),
        Stmt::DoWhile { body, condition } => {
            expr_is_pure(condition) && body.iter().all(stmt_is_pure)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_deref().is_none_or(stmt_is_pure)
                && condition.as_ref().is_none_or(expr_is_pure)
                && update.as_ref().is_none_or(expr_is_pure)
                && body.iter().all(stmt_is_pure)
        }
        Stmt::Labeled { body, .. } => stmt_is_pure(body),
        // Break/Continue are control flow; they don't add side effects
        // but they also mean the body's analysis has to assume the
        // surrounding loop's structure may not run linearly. Safe to
        // treat as pure — a loop whose body only does break/continue
        // and pure ops is still observably empty.
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
        // Conservative for everything else (Try with catch can run
        // arbitrary code; Switch can have any case body).
        _ => false,
    }
}

/// Collect every `Stmt::Let { id }` declared directly in `body` (i.e., at
/// the statement-list level, or inside nested control flow that shares the
/// loop's scope — `If` / `For` init / inner loops). Closure bodies are
/// *not* walked, since their locals belong to a different function scope.
fn collect_body_declared_locals(body: &[Stmt], out: &mut HashSet<u32>) {
    for s in body {
        match s {
            Stmt::Let { id, .. } => {
                out.insert(*id);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_body_declared_locals(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_body_declared_locals(eb, out);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_body_declared_locals(std::slice::from_ref(i), out);
                }
                collect_body_declared_locals(body, out);
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_body_declared_locals(body, out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_body_declared_locals(body, out);
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        out.insert(*id);
                    }
                    collect_body_declared_locals(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_body_declared_locals(f, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    collect_body_declared_locals(&c.body, out);
                }
            }
            Stmt::Labeled { body, .. } => {
                collect_body_declared_locals(std::slice::from_ref(body.as_ref()), out);
            }
            _ => {}
        }
    }
}

/// Return `true` if any `LocalSet` or `Update` inside `body` targets an id
/// NOT in `body_locals` — i.e., the body writes to a local declared in an
/// enclosing scope. Writes to body-declared locals are ignored because they
/// go out of scope at loop exit and can't be observed afterward.
fn body_writes_outside(body: &[Stmt], body_locals: &HashSet<u32>) -> bool {
    body.iter().any(|s| stmt_writes_outside(s, body_locals))
}

fn stmt_writes_outside(s: &Stmt, body_locals: &HashSet<u32>) -> bool {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) => expr_writes_outside(e, body_locals),
        Stmt::Let { init, .. } => init
            .as_ref()
            .is_some_and(|e| expr_writes_outside(e, body_locals)),
        Stmt::Return(opt) => opt
            .as_ref()
            .is_some_and(|e| expr_writes_outside(e, body_locals)),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_writes_outside(condition, body_locals)
                || body_writes_outside(then_branch, body_locals)
                || else_branch
                    .as_ref()
                    .is_some_and(|eb| body_writes_outside(eb, body_locals))
        }
        Stmt::While { condition, body } => {
            expr_writes_outside(condition, body_locals) || body_writes_outside(body, body_locals)
        }
        Stmt::DoWhile { body, condition } => {
            body_writes_outside(body, body_locals) || expr_writes_outside(condition, body_locals)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_deref()
                .is_some_and(|s| stmt_writes_outside(s, body_locals))
                || condition
                    .as_ref()
                    .is_some_and(|e| expr_writes_outside(e, body_locals))
                || update
                    .as_ref()
                    .is_some_and(|e| expr_writes_outside(e, body_locals))
                || body_writes_outside(body, body_locals)
        }
        Stmt::Labeled { body, .. } => stmt_writes_outside(body, body_locals),
        _ => false,
    }
}

fn expr_writes_outside(e: &Expr, body_locals: &HashSet<u32>) -> bool {
    match e {
        Expr::LocalSet(id, value) => {
            !body_locals.contains(id) || expr_writes_outside(value, body_locals)
        }
        Expr::Update { id, .. } => !body_locals.contains(id),
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            expr_writes_outside(left, body_locals) || expr_writes_outside(right, body_locals)
        }
        Expr::Unary { operand, .. } | Expr::Void(operand) | Expr::TypeOf(operand) => {
            expr_writes_outside(operand, body_locals)
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_writes_outside(condition, body_locals)
                || expr_writes_outside(then_expr, body_locals)
                || expr_writes_outside(else_expr, body_locals)
        }
        _ => false,
    }
}

fn expr_is_pure(e: &Expr) -> bool {
    match e {
        // Literals and pure reads.
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::BigInt(_)
        | Expr::String(_)
        | Expr::This
        | Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::FuncRef(_)
        | Expr::ClassRef(_)
        | Expr::EnumMember { .. } => true,

        // Local mutations are pure at the LLVM level (alloca-promoted).
        // GlobalSet writes to a module global and IS observable.
        Expr::LocalSet(_, val) => expr_is_pure(val),

        // HIR's Update variant only ever targets a local (`id: LocalId`),
        // so it is always pure at the LLVM level. PropertyUpdate /
        // IndexUpdate live in their own variants and fall through to
        // the catch-all below.
        Expr::Update { .. } => true,

        // Pure arithmetic / logical / comparison ops.
        Expr::Binary { left, right, .. } => expr_is_pure(left) && expr_is_pure(right),
        Expr::Unary { operand, .. } => expr_is_pure(operand),
        Expr::Compare { left, right, .. } => expr_is_pure(left) && expr_is_pure(right),
        Expr::Logical { left, right, .. } => expr_is_pure(left) && expr_is_pure(right),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => expr_is_pure(condition) && expr_is_pure(then_expr) && expr_is_pure(else_expr),
        Expr::TypeOf(operand) => expr_is_pure(operand),
        Expr::Void(operand) => expr_is_pure(operand),

        // Anything that calls a function, allocates, mutates the heap,
        // throws, or interacts with the runtime is conservatively a
        // side effect. The catch-all matters most: if a future HIR
        // variant escapes here, we'd rather miss the optimization than
        // wrongly insert a barrier and surprise the user.
        _ => false,
    }
}
