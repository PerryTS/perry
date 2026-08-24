//! Object-literal facts recorded while lowering immutable local bindings.

use crate::expr::FnCtx;

/// #5271: recognize both data-only object literals and method/getter IIFEs so
/// own members win over built-in prototype methods during lowering.
pub(super) fn is_object_literal_init(init: &perry_hir::Expr) -> bool {
    use perry_hir::Expr;
    match init {
        Expr::Object(_) => true,
        Expr::Call { callee, args, .. } => {
            (matches!(args.first(), Some(Expr::Object(_)))
                || matches!(
                    args.first(),
                    Some(Expr::New { class_name, .. })
                        if class_name.starts_with("__AnonShape_")
                ))
                && matches!(
                    callee.as_ref(),
                    Expr::Closure { params, .. }
                        if params.first().is_some_and(|p| p.name == "__perry_obj_iife")
                )
        }
        _ => false,
    }
}

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
