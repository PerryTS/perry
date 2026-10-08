//! Synchronous array consumers use one IteratorRecord loop.
use super::{stmt_loops::lower_stmt_for_of_inner, LoweringContext};
use crate::Module;
use anyhow::Result;
use swc_ecma_ast as ast;
pub(crate) fn lower_stmt_for_of(
    ctx: &mut LoweringContext,
    module: &mut Module,
    stmt: &ast::ForOfStmt,
) -> Result<()> {
    lower_stmt_for_of_inner(ctx, module, stmt)
}
