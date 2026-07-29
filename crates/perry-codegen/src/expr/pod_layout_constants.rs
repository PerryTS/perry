use anyhow::{bail, Result};
use perry_hir::types::Type;
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::native_value::{layout_decision_for_type, PodLayoutDecision, PodLayoutManifest};

use super::FnCtx;

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::PodLayoutSizeOf { ty } => {
            let layout = pod_layout_for_constant(ctx, "sizeof", ty)?;
            Ok(double_literal(layout.size as f64))
        }
        Expr::PodLayoutAlignOf { ty } => {
            let layout = pod_layout_for_constant(ctx, "alignof", ty)?;
            Ok(double_literal(layout.alignment as f64))
        }
        Expr::PodLayoutOffsetOf { ty, field_path } => {
            let layout = pod_layout_for_constant(ctx, "offsetof", ty)?;
            let field = layout
                .fields
                .iter()
                .find(|field| field.path == *field_path)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "offsetof<T>(\"{}\") could not find that field path in the resolved PerryPod layout",
                        field_path.join(".")
                    )
                })?;
            Ok(double_literal(field.offset as f64))
        }
        other => bail!("pod layout constant lowering called for {:?}", other),
    }
}

fn pod_layout_for_constant(
    ctx: &FnCtx<'_>,
    intrinsic: &str,
    ty: &Type,
) -> Result<PodLayoutManifest> {
    match layout_decision_for_type(ctx, ty) {
        PodLayoutDecision::Layout(layout) => Ok(layout),
        PodLayoutDecision::Rejected(reason) => bail!(
            "{}<T>() requires T to resolve to an accepted PerryPod<...> layout: {}",
            intrinsic,
            reason
        ),
        PodLayoutDecision::NotPod => bail!(
            "{}<T>() requires T to resolve to PerryPod<...>; got {:?}",
            intrinsic,
            ty
        ),
    }
}
