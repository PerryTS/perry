//! Private-member (`#field`) brand-guard wrapping for member lowering.
//!
//! Split out of `expr_member.rs` (pure code move).

use crate::ir::Expr;

use super::LoweringContext;

/// Wire codes for `Expr::PrivateGuard.op` — the operation a private member
/// access performs. Keep in sync with the `js_private_guard` runtime helper:
/// 0/1 are instance read/write, 2/3 are static read/write.
pub(crate) const PRIV_OP_READ: u8 = 0;
pub(crate) const PRIV_OP_WRITE: u8 = 1;

/// Return the physical property key used for a private field value. Private
/// methods and accessors live in the class registry and keep their source
/// spelling for dispatch, but field values need a key that cannot collide with
/// an ordinary computed property such as `["#x"]`.
pub(crate) fn private_storage_property(ctx: &LoweringContext, field_name: &str) -> String {
    match ctx.resolve_private(field_name) {
        Some((_, class_id, member)) => {
            let family = if member.kind == super::super::PrivKind::Field {
                "value"
            } else {
                "member"
            };
            format!("#<perry:private-{family}:{class_id}:{field_name}>")
        }
        None => field_name.to_string(),
    }
}

pub(crate) fn is_class_expr_self_binding(ctx: &LoweringContext, object: &Expr) -> bool {
    matches!(
        object,
        Expr::LocalGet(id)
            if ctx.class_expr_self_bindings.iter().any(|(_, _, binding_id)| binding_id == id)
    )
}

/// Wrap the receiver of a private member access `obj.#name` in a brand+kind
/// guard so an access on a non-conforming receiver throws `TypeError`. If the
/// name cannot be resolved to a declaring class in scope, the object is
/// returned unwrapped (falls back to the pre-existing string-keyed behavior so
/// this can never reject a legal access). A STATIC member emits a static-brand
/// guard (the receiver must be the declaring class constructor itself).
/// `op` is `PRIV_OP_READ` / `PRIV_OP_WRITE`.
pub(crate) fn wrap_private_guard(
    ctx: &LoweringContext,
    object: Box<Expr>,
    field_name: &str,
    op: u8,
) -> Box<Expr> {
    if let Some((class_name, class_id, member)) = ctx.resolve_private(field_name) {
        // Static members get a static brand (op + 2); instance members the
        // ordinary op code.
        let op = if member.is_static { op + 2 } else { op };
        let receiver_is_brand_owner = is_class_expr_self_binding(ctx, &object);
        return Box::new(Expr::PrivateGuard {
            class_name,
            class_id,
            field_name: field_name.to_string(),
            kind: member.kind as u8,
            op,
            receiver_is_brand_owner,
            object,
        });
    }
    object
}
