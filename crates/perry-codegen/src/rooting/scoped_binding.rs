//! Root ownership for lexical expression bindings. Keep raw slot handles and
//! their push/read/release protocol inside the rooting module.

use anyhow::Result;
use perry_hir::{types::LocalId, Expr};

use super::temp_root;
use crate::expr::{lower_expr, FnCtx};

/// Evaluate the initializer once, protect it for the whole continuation, and
/// release its slot on both successful and failed lowering paths.
pub(crate) fn lower_scoped_binding(
    ctx: &mut FnCtx<'_>,
    id: LocalId,
    value: &Expr,
    body: &Expr,
) -> Result<String> {
    let value = lower_expr(ctx, value)?;
    let root = temp_root::temp_root_push_double(ctx, &value);
    ctx.scoped_temp_roots.push((id, root.clone()));
    let result = lower_expr(ctx, body);
    ctx.scoped_temp_roots.pop();
    temp_root::temp_root_truncate(ctx, &root);
    result
}

/// Lower a bound local read from its current, possibly relocated slot. As for
/// ordinary LocalGet, enclosing expression combinators protect the resulting
/// register if a subsequent sibling can collect. Callers never own the slot.
pub(crate) fn read_scoped_binding(ctx: &mut FnCtx<'_>, id: LocalId) -> Option<String> {
    let root = ctx
        .scoped_temp_roots
        .iter()
        .rev()
        .find(|(temp, _)| *temp == id)?
        .1
        .clone();
    Some(temp_root::temp_root_get_double(ctx, &root))
}
