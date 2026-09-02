//! Receiver-classification helpers for the dynamic instance-method
//! lowering, split out of `dynamic_dispatch.rs` for the 2000-line cap.

use crate::expr::FnCtx;
use perry_hir::Expr;

/// Can an exact canonical shape prove that `property` is not an own field?
///
/// A post-construction assignment such as `this.run = f` mints a successor
/// ShapeId, so an exact canonical-shape match excludes that override. A
/// declared field is different: it is already part of the canonical shape and
/// may intentionally shadow a prototype method (#620). Computed fields and
/// incomplete/dynamic parent chains are similarly unknowable here and retain
/// the runtime own-property probe.
pub(super) fn canonical_shape_excludes_own_property(
    ctx: &FnCtx<'_>,
    class_name: &str,
    property: &str,
) -> bool {
    let mut current = Some(class_name.to_string());
    let mut seen = std::collections::HashSet::new();
    while let Some(name) = current {
        if !seen.insert(name.clone()) {
            return false;
        }
        let Some(class) = ctx.classes.get(&name) else {
            return false;
        };
        if class
            .fields
            .iter()
            .any(|field| field.key_expr.is_some() || field.name == property)
        {
            return false;
        }
        if class.extends_expr.is_some() || class.native_extends.is_some() {
            return false;
        }
        current = class.extends_name.clone().or_else(|| {
            class.extends.and_then(|parent_id| {
                ctx.classes
                    .iter()
                    .find_map(|(name, candidate)| (candidate.id == parent_id).then(|| name.clone()))
            })
        });
    }
    true
}

/// A declared class may select the direct-method guard, but never prove the
/// direct call. The guard validates the live class id, keys token, own
/// override, and resolved method pointer; every miss uses dynamic dispatch.
pub(super) fn guarded_declared_receiver_class_candidate(
    ctx: &FnCtx<'_>,
    object: &Expr,
) -> Option<String> {
    let Expr::LocalGet(id) = object else {
        return None;
    };
    let perry_hir::types::Type::Named(name) = ctx.local_type_hint(id)? else {
        return None;
    };
    ctx.classes.contains_key(name).then(|| name.clone())
}
