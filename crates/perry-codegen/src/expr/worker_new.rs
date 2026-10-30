//! Worker construction keeps the original source value until runtime validation.
use crate::expr::FnCtx;
use crate::nanbox::double_literal;
use crate::rooting::with_rooted_group;
use crate::types::{DOUBLE, I64, VOID};
use anyhow::{anyhow, bail, Result};
use perry_hir::Expr;

pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    paths: &[String],
    filename: &Expr,
    options: Option<&Expr>,
    is_eval: bool,
) -> Result<String> {
    with_rooted_group(ctx, 2, |ctx, roots| {
        let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        let file = roots.lower(ctx, filename, true)?;
        if ctx.block().is_terminated() {
            return Ok(undefined);
        }
        let opts = roots.lower(ctx, options.unwrap_or(&Expr::Undefined), true)?;
        if ctx.block().is_terminated() {
            return Ok(undefined);
        }
        let entry = if is_eval && paths.len() == 1 {
            let target = ctx
                .dynamic_import_path_to_prefix
                .get(&paths[0])
                .cloned()
                .ok_or_else(|| {
                    anyhow!(
                        "worker_threads Worker target was not compiled: {}",
                        paths[0]
                    )
                })?;
            if target.starts_with("__node_submod__") || target.starts_with("__native_mod__") {
                bail!("worker_threads Worker target must be a compiled source file");
            }
            let init = if ctx.program_has_worker {
                format!("{target}__init")
            } else {
                format!("{target}__init_body")
            };
            ctx.pending_declares.push((init.clone(), VOID, vec![]));
            ctx.block().ptrtoint(&format!("@{init}"), I64)
        } else {
            "0".into()
        };
        let file = roots.reread(ctx, file)?;
        let opts = roots.reread(ctx, opts)?;
        Ok(ctx.block().call(
            DOUBLE,
            "js_worker_threads_worker_new_with_source",
            &[(DOUBLE, &file), (DOUBLE, &opts), (I64, &entry)],
        ))
    })
}
