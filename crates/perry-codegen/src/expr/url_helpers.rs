//! Scalar URL getter lowering (extracted from `expr.rs`, issue #1098).
//! Pure move — no logic changes.

use anyhow::Result;
use perry_hir::Expr;

use super::{lower_expr, unbox_to_i64, FnCtx};
use crate::types::{DOUBLE, I64};

/// Lower one of the scalar URL getters (`url.href`, `url.pathname`, …).
/// Each runtime entry takes a raw `*mut ObjectHeader` and returns an
/// already NaN-boxed f64 string, so the caller only has to unbox the
/// URL handle.
pub(crate) fn lower_url_string_getter(
    ctx: &mut FnCtx<'_>,
    url: &Expr,
    runtime_fn: &str,
) -> Result<String> {
    let v = lower_expr(ctx, url)?;
    let obj_ptr = unbox_to_i64(ctx.block(), &v);
    Ok(ctx.block().call(DOUBLE, runtime_fn, &[(I64, &obj_ptr)]))
}
