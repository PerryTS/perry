//! Async-generator `yield`-operand awaiting rewrite, split out of `lower.rs`.

use super::*;

/// For async generators, `yield E` evaluates as `AsyncGeneratorYield(?
/// Await(E))` — the operand is awaited (one microtask tick) before being
/// delivered to the consumer. So `yield Promise.reject(x)` awaits the rejection
/// and throws `x` into the generator, and `yield Promise.resolve(v)` yields `v`,
/// not the promise. Perry yielded the raw operand. This pass rewrites every
/// statement-level non-delegate `yield E` (the only positions left after
/// `hoist_yields`) into `let __ayield = await E; yield __ayield`. The `await`
/// lowers to its own suspension state via the existing await machinery; the temp
/// is a cross-state local that `collect_hoisted_vars` boxes. `yield*` delegation
/// is left untouched — it awaits each delegated step through `delegate_await`.
pub(crate) fn await_async_generator_yield_operands(stmts: &mut Vec<Stmt>, next_id: &mut LocalId) {
    let mut out: Vec<Stmt> = Vec::with_capacity(stmts.len());
    for mut stmt in std::mem::take(stmts) {
        // Recurse into nested control-flow bodies first (mirrors
        // `collect_vars_recursive`). Nested closures are not descended — their
        // yields belong to inner generators.
        match &mut stmt {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                await_async_generator_yield_operands(then_branch, next_id);
                if let Some(eb) = else_branch {
                    await_async_generator_yield_operands(eb, next_id);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                await_async_generator_yield_operands(body, next_id);
            }
            Stmt::For { body, .. } => await_async_generator_yield_operands(body, next_id),
            Stmt::Labeled { body, .. } => {
                let mut wrapped = vec![std::mem::replace(body.as_mut(), Stmt::Break)];
                await_async_generator_yield_operands(&mut wrapped, next_id);
                // A labeled statement wraps a single loop/block (never a bare
                // yield), so the rewrite only touches its inner body and the
                // wrapper stays a single statement.
                if let Some(inner) = wrapped.pop() {
                    *body.as_mut() = inner;
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                await_async_generator_yield_operands(body, next_id);
                if let Some(c) = catch {
                    await_async_generator_yield_operands(&mut c.body, next_id);
                }
                if let Some(f) = finally {
                    await_async_generator_yield_operands(f, next_id);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    await_async_generator_yield_operands(&mut case.body, next_id);
                }
            }
            _ => {}
        }

        // Pull the non-delegate yield operand into a preceding `await`.
        let yield_value: Option<&mut Option<Box<Expr>>> = match &mut stmt {
            Stmt::Expr(Expr::Yield {
                value,
                delegate: false,
            }) => Some(value),
            Stmt::Let {
                init:
                    Some(Expr::Yield {
                        value,
                        delegate: false,
                    }),
                ..
            } => Some(value),
            Stmt::Return(Some(Expr::Yield {
                value,
                delegate: false,
            })) => Some(value),
            _ => None,
        };
        if let Some(value) = yield_value {
            let operand = value.take().map(|b| *b).unwrap_or(Expr::Undefined);
            let tmp = alloc_local(next_id);
            *value = Some(Box::new(Expr::LocalGet(tmp)));
            out.push(Stmt::Let {
                id: tmp,
                name: format!("__ayield_{}", tmp),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Await(Box::new(operand))),
            });
        }
        out.push(stmt);
    }
    *stmts = out;
}
