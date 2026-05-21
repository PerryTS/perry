//! `<inst>.exports.<method>(args)` for WebAssembly JS API (#76).
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

use super::super::{
    extract_typed_parse_source_order, is_generator_call_expr, is_widget_modifier_name, lower_expr,
    resolve_typed_parse_ty, LoweringContext,
};

pub(super) fn try_wasm_instance_exports(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    expr: &ast::Expr,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    // Issue #76 — standard `<inst>.exports.<method>(args...)` shape
    // from the WebAssembly JS API. Sits OUTSIDE the Ident-receiver
    // gate below because `<inst>.exports` is itself a Member, not
    // an Ident. Only routes when `<inst>` resolves to a tagged
    // wasm-instance local (populated at var-decl time when the
    // initializer is `WebAssembly.instantiate(...)`) — this avoids
    // stealing `module.exports.foo()` etc. from the generic dispatch.
    if let ast::Expr::Member(outer_member) = expr {
        if let ast::MemberProp::Ident(method_ident) = &outer_member.prop {
            if let ast::Expr::Member(inner) = outer_member.obj.as_ref() {
                if let ast::MemberProp::Ident(inner_prop) = &inner.prop {
                    if inner_prop.sym.as_ref() == "exports" {
                        if let ast::Expr::Ident(inst_ident) = inner.obj.as_ref() {
                            let inst_name = inst_ident.sym.as_ref();
                            if ctx.wasm_instance_locals.contains(inst_name) {
                                let instance_lowered = lower_expr(ctx, inner.obj.as_ref())?;
                                ctx.uses_webassembly = true;
                                return Ok(Ok(Expr::WebAssemblyCallExport {
                                    instance: Box::new(instance_lowered),
                                    name: Box::new(Expr::String(
                                        method_ident.sym.as_ref().to_string(),
                                    )),
                                    args,
                                }));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(Err(args))
}
