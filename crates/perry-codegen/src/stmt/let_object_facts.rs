//! Object-literal facts recorded while lowering immutable local bindings.

use crate::expr::FnCtx;

/// Propagate a producer-proven imported object through immutable local aliases.
/// Mutable or reassigned bindings remain generic so replacement stays visible.
pub(super) fn record_imported_object_alias(
    ctx: &mut FnCtx<'_>,
    id: u32,
    init: Option<&perry_hir::Expr>,
    mutable: bool,
) {
    let binding = (!mutable && !ctx.reassigned_locals.contains(&id))
        .then(|| match init {
            Some(perry_hir::Expr::ExternFuncRef { name, .. })
                if ctx.imported_object_literals.contains_key(name) =>
            {
                Some(name.clone())
            }
            Some(perry_hir::Expr::LocalGet(source_id)) => {
                ctx.local_imported_object_aliases.get(source_id).cloned()
            }
            _ => None,
        })
        .flatten();
    if let Some(binding) = binding {
        ctx.local_imported_object_aliases.insert(id, binding);
    } else {
        ctx.local_imported_object_aliases.remove(&id);
    }
}
