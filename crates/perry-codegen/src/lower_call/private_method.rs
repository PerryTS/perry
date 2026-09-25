//! Fused instance `recv.#m(args)` (#10501).
//!
//! The generic lowering of a private method call READ the method first — the
//! callee `PropertyGet { object: PrivateGuard, .. }` ran `js_private_guard`,
//! which recorded a member hint, and the by-name get consumed that hint and
//! allocated a bound-method closure — and then called the closure: roughly
//! 12,000 instructions per call. A private method cannot be overridden,
//! shadowed or replaced, so this lowering keeps the two observable steps and
//! drops the rest:
//!
//! 1. `js_private_method_guard` brand-checks the receiver BEFORE the arguments
//!    are evaluated (PrivateGet throws before any argument side effect), and
//! 2. `js_private_method_call` calls the declaring class's vtable entry AFTER
//!    them, resolving it once per site.
//!
//! Neither step uses the member-hint stack. Static private methods (`op` 2)
//! and an unresolved declaring class keep the generic lowering.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{emit_private_site_cache, emit_string_literal_global, lower_expr, FnCtx};
use crate::rooting::{any_operand_may_collect, with_rooted_group, Repr};
use crate::types::{DOUBLE, I32, I64, PTR};

/// `Expr::PrivateGuard` wire codes (see `perry_hir::Expr::PrivateGuard`):
/// kind 1 is a method, op 0 an instance read.
const PRIVATE_KIND_METHOD: u8 = 1;
const PRIVATE_OP_INSTANCE_READ: u8 = 0;

pub(super) fn try_lower_private_method_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    let Expr::PropertyGet { object, .. } = callee else {
        return Ok(None);
    };
    let Expr::PrivateGuard {
        class_name,
        class_id,
        field_name,
        kind,
        op,
        receiver_is_brand_owner,
        object: receiver,
    } = object.as_ref()
    else {
        return Ok(None);
    };
    if *kind != PRIVATE_KIND_METHOD || *op != PRIVATE_OP_INSTANCE_READ {
        return Ok(None);
    }
    // Same declaring-class resolution as the `PrivateGuard` arm: the node's
    // own id, falling back to the (name-keyed, collision-prone) map only when
    // the node carries none.
    let class_id = if *class_id != 0 {
        *class_id
    } else {
        ctx.class_ids.get(class_name).copied().unwrap_or(0)
    };
    if class_id == 0 {
        return Ok(None);
    }
    let receiver_is_brand_owner = *receiver_is_brand_owner;
    let call_byte_offset = ctx.strings.pending_call_offset();
    let class_id_str = class_id.to_string();
    let name_len_str = field_name.len().to_string();

    with_rooted_group(ctx, args.len(), |ctx, group| {
        let recv = lower_expr(ctx, receiver)?;
        // The rooting window between these two operands is empty: lowering
        // `this` only reads the current binding and cannot collect.
        let brand_owner = if receiver_is_brand_owner {
            recv.clone()
        } else {
            lower_expr(ctx, &Expr::This)?
        };
        let name_label = emit_string_literal_global(ctx, field_name);
        let guard_site = emit_private_site_cache(ctx, 1);
        let guarded = ctx.block().call(
            DOUBLE,
            "js_private_method_guard",
            &[
                (DOUBLE, &recv),
                (DOUBLE, &brand_owner),
                (I32, &class_id_str),
                (PTR, &name_label),
                (I32, &name_len_str),
                (PTR, &guard_site),
            ],
        );
        // Root the guarded receiver across the arguments, each argument
        // across the ones after it, then re-read everything below the last
        // collection point.
        let guarded = group.adopt_emitted(
            ctx,
            Repr::Boxed,
            &guarded,
            any_operand_may_collect(ctx, args.iter()),
        );
        for (i, arg) in args.iter().enumerate() {
            let collects = any_operand_may_collect(ctx, args[i + 1..].iter());
            group.lower(ctx, arg, collects)?;
        }
        let lowered_args = group.reread_all(ctx)?;
        let recv = group.reread_emitted(ctx, guarded);
        let brand_owner = if receiver_is_brand_owner {
            recv.clone()
        } else {
            lower_expr(ctx, &Expr::This)?
        };
        let (args_ptr, args_len) = if lowered_args.is_empty() {
            ("null".to_string(), "0".to_string())
        } else {
            // Entry-block alloca: an alloca inside a loop body is a stack
            // adjustment that is never restored (#167).
            let buf = ctx.func.alloca_entry_array(DOUBLE, lowered_args.len());
            let blk = ctx.block();
            for (i, value) in lowered_args.iter().enumerate() {
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
                blk.store(DOUBLE, value, &slot);
            }
            (buf, lowered_args.len().to_string())
        };
        let call_site = emit_private_site_cache(ctx, 2);
        crate::expr::calls::emit_call_location_at(ctx, call_byte_offset);
        Ok(Some(ctx.block().call(
            DOUBLE,
            "js_private_method_call",
            &[
                (DOUBLE, &recv),
                (DOUBLE, &brand_owner),
                (I32, &class_id_str),
                (PTR, &name_label),
                (I32, &name_len_str),
                (PTR, &args_ptr),
                (I64, &args_len),
                (PTR, &call_site),
            ],
        )))
    })
}
