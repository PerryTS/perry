use anyhow::{anyhow, bail, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::analysis::*;
use crate::destructuring::*;
use crate::ir::*;
use crate::lower::{
    collect_for_of_pattern_leaves, emit_for_of_pattern_binding, lower_expr, LoweringContext,
};
use crate::lower_patterns::*;
use crate::lower_types::*;

use super::*;

pub fn lower_type_alias_decl(
    ctx: &mut LoweringContext,
    alias_decl: &ast::TsTypeAliasDecl,
    is_exported: bool,
) -> Result<TypeAlias> {
    let name = alias_decl.id.sym.to_string();
    let alias_id = ctx.fresh_type_alias();

    // Extract type parameters
    let type_params = alias_decl
        .type_params
        .as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    // Enter type param scope for resolving type references
    ctx.enter_type_param_scope(&type_params);

    // Extract the aliased type
    let ty = extract_ts_type_with_ctx(&alias_decl.type_ann, Some(ctx));

    ctx.exit_type_param_scope();

    // Register type alias in context
    ctx.type_aliases
        .push((name.clone(), alias_id, type_params.clone(), ty.clone()));

    Ok(TypeAlias {
        id: alias_id,
        name,
        type_params,
        ty,
        is_exported,
    })
}
