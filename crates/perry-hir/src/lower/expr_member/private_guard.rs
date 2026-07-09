//! Private-member (`#field`) brand-guard wrapping for member lowering.
//!
//! Split out of `expr_member.rs` (pure code move).

use anyhow::Result;
use perry_types::Type;
use swc_common::Spanned;
use swc_ecma_ast as ast;

use crate::ir::Expr;

use super::{lower_expr, LoweringContext};

use super::*;

/// Wire codes for `Expr::PrivateGuard.op` — the operation a private member
/// access performs. Keep in sync with the `js_private_guard` runtime helper:
/// 0/1 are instance read/write, 2/3 are static read/write.
pub(crate) const PRIV_OP_READ: u8 = 0;
pub(crate) const PRIV_OP_WRITE: u8 = 1;

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
        return Box::new(Expr::PrivateGuard {
            class_name,
            class_id,
            field_name: field_name.to_string(),
            kind: member.kind as u8,
            op,
            object,
        });
    }
    object
}
