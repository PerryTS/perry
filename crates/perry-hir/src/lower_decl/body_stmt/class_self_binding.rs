//! #11157: the class-declaration arm's half of the per-evaluation self-binding
//! (see `lower_decl/class_decl/decl_self_binding.rs` for the lowering half).

use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::{Class, Expr};
use crate::lower::LoweringContext;
use crate::types::LocalId;

/// Lower a function-body class declaration, asking `lower_class_decl` for a
/// self-binding. Returns the class and the self-binding local, if one was
/// registered. The binding stack is restored even when lowering fails.
pub(super) fn lower_body_class_decl(
    ctx: &mut LoweringContext,
    class_decl: &ast::ClassDecl,
) -> Result<(Class, Option<LocalId>)> {
    let mark = ctx.class_expr_self_bindings.len();
    ctx.class_decl_self_binding_wanted = true;
    let class = super::lower_class_decl(ctx, class_decl, false);
    ctx.class_decl_self_binding_wanted = false;
    ctx.class_expr_self_bindings.truncate(mark);
    let self_binding = ctx.class_decl_self_binding.take();
    Ok((class?, self_binding))
}

/// The `ClassExprFresh` evaluation owner for a declaration: its self-binding,
/// when anything the evaluation runs reads it. The owner local is registered
/// with the enclosing body so it is declared (and shadow-rooted) at body
/// entry, before codegen stores the evaluated class object in it — the same
/// contract a named class expression's self-binding has (`arm_class.rs`).
pub(super) fn decl_self_binding_owner(
    ctx: &mut LoweringContext,
    self_binding: Option<LocalId>,
    class_name: &str,
    captured_exprs: &[Expr],
    named_statics: &[(String, Expr)],
    computed_keys: &[(String, Expr)],
    computed_statics: &[(String, Expr)],
) -> Option<LocalId> {
    let self_id = self_binding?;
    let uses_self = |expr: &Expr| {
        let mut refs = Vec::new();
        let mut visited = std::collections::HashSet::new();
        crate::analysis::collect_local_refs_expr(expr, &mut refs, &mut visited);
        refs.contains(&self_id)
    };
    let used = captured_exprs.iter().any(&uses_self)
        || named_statics.iter().any(|(_, value)| uses_self(value))
        || computed_keys.iter().any(|(_, key)| uses_self(key))
        || computed_statics.iter().any(|(_, value)| uses_self(value));
    if !used {
        return None;
    }
    let ids = ctx
        .lookup_class_captures(class_name)
        .map(<[_]>::to_vec)
        .unwrap_or_default();
    ctx.body_class_expr_captures.push((self_id, ids));
    Some(self_id)
}
