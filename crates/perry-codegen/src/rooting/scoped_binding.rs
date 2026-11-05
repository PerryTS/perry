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
    lower_scoped_body(ctx, id, &value, body)
}

/// The body half of [`lower_scoped_binding`], for a value the caller lowered
/// itself (an optional method call bound for the rest of its chain).
pub(crate) fn lower_scoped_body(
    ctx: &mut FnCtx<'_>,
    id: LocalId,
    value: &str,
    body: &Expr,
) -> Result<String> {
    let root = temp_root::temp_root_push_double(ctx, value);
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

/// Capture construct arguments before a heritage guard. Both continuations
/// read the same rooted values; argument side effects and GC run exactly once.
pub(crate) fn with_scoped_arguments(
    ctx: &mut FnCtx<'_>,
    args: &[Expr],
    body: impl FnOnce(&mut FnCtx<'_>, &[Expr]) -> Result<String>,
) -> Result<String> {
    let mut roots = Vec::new();
    let mut reads = Vec::new();
    let mut proofs = Vec::new();
    let result = (|| {
        for arg in args {
            let proof = crate::type_analysis::proven_type_from_init(ctx, arg);
            let value = lower_expr(ctx, arg)?;
            let root = temp_root::temp_root_push_double(ctx, &value);
            let mut id = u32::MAX - ctx.scoped_temp_roots.len() as u32;
            while ctx.locals.contains_key(&id)
                || ctx.scoped_temp_roots.iter().any(|(bound, _)| *bound == id)
            {
                id -= 1;
            }
            let previous = ctx.proven_local_types.remove(&id);
            if let Some(proof) = proof {
                ctx.proven_local_types.insert(id, proof);
            }
            proofs.push((id, previous));
            ctx.scoped_temp_roots.push((id, root.clone()));
            roots.push(root);
            reads.push(Expr::LocalGet(id));
        }
        body(ctx, &reads)
    })();
    for (root, (id, previous)) in roots.iter().zip(proofs).rev() {
        ctx.scoped_temp_roots.pop();
        ctx.proven_local_types.remove(&id);
        if let Some(previous) = previous {
            ctx.proven_local_types.insert(id, previous);
        }
        temp_root::temp_root_truncate(ctx, root);
    }
    result
}
