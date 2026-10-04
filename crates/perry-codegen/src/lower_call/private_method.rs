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

use crate::expr::{
    emit_private_site_cache, emit_private_site_guard, emit_string_literal_global, lower_expr, FnCtx,
};
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
    let direct_target = private_method_direct_target(ctx, class_name, class_id, field_name);
    // The declaring class's completed static shape carries its brand.
    let static_id =
        crate::codegen::static_private_class::class_static_private_final(ctx, class_name)
            .map(|(id, _)| id);
    // #11791: inside the proven-`this` clone of the declaring class, `this` is
    // exactly that class (the clone's call-site guard matched its class id
    // and a shape it accepts), so `this.#m()` calls `#m`'s own clone when one
    // was emitted, as `this.m()` does.
    let direct_target = direct_target.map(|(symbol, params)| {
        let proven = matches!(receiver.as_ref(), Expr::This)
            && ctx
                .proven_this
                .as_ref()
                .is_some_and(|fact| fact.class_name == *class_name)
            && ctx
                .pshape_methods
                .contains_key(&(class_name.clone(), field_name.clone()));
        if proven {
            (crate::collectors::pshape_method_name(&symbol), params)
        } else {
            (symbol, params)
        }
    });

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
        let guarded =
            emit_private_site_guard(ctx, &recv, class_id, &guard_site, static_id, |ctx| {
                ctx.block().call(
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
                )
            });
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
        let runtime_call = |ctx: &mut FnCtx<'_>| {
            ctx.block().call(
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
            )
        };
        let Some((direct_fn, param_count)) = direct_target else {
            return Ok(Some(runtime_call(ctx)));
        };
        // The guard above proved the receiver carries the class's brand. While
        // the template has no fresh evaluation (re-tested here: an argument may
        // have evaluated the class again), the method `#m` resolves to is the
        // template's own compiled body, which nothing can replace, so it is
        // called directly; otherwise the runtime resolves the evaluation's.
        let direct_idx = ctx.new_block("pmcall.direct");
        let slow_idx = ctx.new_block("pmcall.runtime");
        let join_idx = ctx.new_block("pmcall.join");
        let direct_l = ctx.block_label(direct_idx);
        let slow_l = ctx.block_label(slow_idx);
        let join_l = ctx.block_label(join_idx);
        let inert = crate::expr::emit_private_template_inert(ctx.block(), class_id);
        ctx.block().cond_br(&inert, &direct_l, &slow_l);

        ctx.current_block = direct_idx;
        let undefined = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        let mut call_args: Vec<(crate::types::LlvmType, String)> = vec![(DOUBLE, recv.clone())];
        for i in 0..param_count {
            let value = lowered_args
                .get(i)
                .cloned()
                .unwrap_or_else(|| undefined.clone());
            call_args.push((DOUBLE, value));
        }
        let call_args_ref: Vec<(crate::types::LlvmType, &str)> =
            call_args.iter().map(|(t, v)| (*t, v.as_str())).collect();
        let direct = ctx.block().call(DOUBLE, &direct_fn, &call_args_ref);
        let direct_end = ctx.block_label(ctx.current_block);
        ctx.block().br(&join_l);

        ctx.current_block = slow_idx;
        let slow = runtime_call(ctx);
        let slow_end = ctx.block_label(ctx.current_block);
        ctx.block().br(&join_l);

        ctx.current_block = join_idx;
        Ok(Some(ctx.block().phi(
            DOUBLE,
            &[(&direct, &direct_end), (&slow, &slow_end)],
        )))
    })
}

/// The compiled body of instance private method `name` of `class_name`
/// (class id `class_id`) and its declared parameter count, when a call site
/// may call it directly: the class is compiled in this module under that id,
/// and the method takes plain positional parameters (no rest parameter, no
/// `arguments` object), so the call needs no runtime argument packing.
fn private_method_direct_target(
    ctx: &FnCtx<'_>,
    class_name: &str,
    class_id: u32,
    name: &str,
) -> Option<(String, usize)> {
    if ctx.class_ids.get(class_name).copied() != Some(class_id) {
        return None;
    }
    let key = (class_name.to_string(), name.to_string());
    let symbol = ctx.methods.get(&key)?;
    let prefix = ctx.strings.module_prefix();
    if prefix.is_empty() || !symbol.starts_with(&format!("perry_method_{prefix}__")) {
        return None;
    }
    if ctx.method_has_rest.get(&key).copied().unwrap_or(false)
        || ctx
            .method_has_synthetic_arguments
            .get(&key)
            .copied()
            .unwrap_or(false)
        || ctx
            .method_arguments_length_only
            .get(&key)
            .copied()
            .unwrap_or(false)
    {
        return None;
    }
    let param_count = *ctx.method_param_counts.get(&key)?;
    Some((symbol.clone(), param_count))
}
