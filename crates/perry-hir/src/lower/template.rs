//! AST to HIR lowering — extracted from `lower/mod.rs` (issue #1101).
//!
//! Pure mechanical split: no logic changes. Helpers keep their original
//! visibility and are re-exported from `lower/mod.rs` so the existing
//! `expr_*` submodules and the rest of the crate keep compiling unchanged.

use anyhow::Result;
use swc_ecma_ast as ast;

use super::*;
use crate::ir::*;

/// Unescape template literal strings (handle \n, \t, etc.)
fn _unescape_template() {}

/// Lower a template literal AST node to its desugared string-concat HIR
/// expression: `\`pre${x}post\`` → `Expr::Binary(Add, "pre", x) + "post"`.
/// Mirrors the inline Tpl lowering at `ast::Expr::Tpl` — extracted so the
/// reactive-Text desugaring can re-lower the same template twice (once for
/// the initial widget value, once inside the rebuild closure).
pub(crate) fn lower_tpl_to_concat(ctx: &mut LoweringContext, tpl: &ast::Tpl) -> Result<Expr> {
    if tpl.quasis.is_empty() {
        return Ok(Expr::String(String::new()));
    }
    let first_raw = tpl.quasis.first().map(|q| q.raw.as_ref()).unwrap_or("");
    let mut result = Expr::String(unescape_template(first_raw));
    for (i, expr) in tpl.exprs.iter().enumerate() {
        let lowered = lower_expr(ctx, expr)?;
        result = Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(result),
            right: Box::new(lowered),
        };
        if let Some(quasi) = tpl.quasis.get(i + 1) {
            let quasi_str: &str = quasi.raw.as_ref();
            if !quasi_str.is_empty() {
                result = Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(result),
                    right: Box::new(Expr::String(unescape_template(quasi_str))),
                };
            }
        }
    }
    Ok(result)
}
