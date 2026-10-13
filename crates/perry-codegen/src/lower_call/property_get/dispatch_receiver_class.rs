//! Receiver-classification helpers for the dynamic instance-method
//! lowering, split out of `dynamic_dispatch.rs` for the 2000-line cap.

use crate::expr::FnCtx;
use perry_hir::Expr;

/// A declared class may select the direct-method guard, but never prove the
/// direct call. The guard validates the live class id, keys token, own
/// override, and resolved method pointer; every miss uses dynamic dispatch.
pub(super) fn guarded_declared_receiver_class_candidate(
    ctx: &FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Option<String> {
    use perry_hir::types::Type;
    let ty = match object {
        Expr::LocalGet(id) => ctx.local_type_hint(id)?,
        Expr::IndexGet { object, .. }
            if args
                .iter()
                .all(|arg| crate::rooting::expr_is_inert_primitive(ctx, arg)) =>
        {
            // Coercion of an object argument can invalidate the receiver proof
            // inside the callee. Admit only runtime-proven primitive arguments.
            // An erased element annotation selects a candidate, never a proof.
            // Reuse the live class/shape/holder guard used for named locals.
            let Expr::LocalGet(id) = object.as_ref() else {
                return None;
            };
            let Type::Array(element) = ctx.local_type_hint(id)? else {
                return None;
            };
            element.as_ref()
        }
        _ => return None,
    };
    let Type::Named(name) = ty else { return None };
    if matches!(object, Expr::IndexGet { .. })
        && !crate::lower_call::method_shape_proof::can_consume(ctx, name, property, args)
    {
        return None;
    }
    ctx.classes.contains_key(name).then(|| name.clone())
}

/// Only the class-dispatch arm needs the exact callable for a generator birth.
/// An unrelated class declaration must not change builtin lowering. Unknown
/// receivers retain Get + Call when a tower arm could select a generator.
pub(super) fn generator_receiver_candidate(
    ctx: &FnCtx<'_>,
    class: Option<&str>,
    property: &str,
) -> bool {
    let defines = |c: &perry_hir::Class| {
        c.methods
            .iter()
            .chain(
                c.computed_members
                    .iter()
                    .filter(|m| !m.is_static)
                    .map(|m| &m.function),
            )
            .any(|f| {
                f.name == property
                    && crate::codegen::helpers::body_has_fixed_generator_state(&f.body)
            })
    };
    let Some(name) = class.filter(|name| ctx.classes.contains_key(*name)) else {
        return ctx.classes.values().any(|c| defines(c));
    };
    // A declared receiver also admits its subclasses through the existing
    // shape/body guards, so their generator overrides need this route too.
    if ctx
        .class_hierarchy
        .descendants(name, None)
        .iter()
        .any(|name| ctx.classes.get(*name).is_some_and(|c| defines(c)))
    {
        return true;
    }
    let mut current = Some(name);
    for _ in 0..ctx.classes.len() {
        let Some(c) = current.and_then(|name| ctx.classes.get(name)) else {
            break;
        };
        if defines(c) {
            return true;
        }
        current = c.extends_name.as_deref();
    }
    false
}
