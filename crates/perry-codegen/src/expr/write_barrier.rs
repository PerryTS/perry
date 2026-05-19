//! GC write-barrier emission helpers + stream-subclass `super(...)`
//! lowering (extracted from `expr.rs`, issue #1098). Pure move — no
//! logic changes.

use anyhow::Result;
use perry_hir::Expr;

use super::{lower_expr, FnCtx};
use crate::block::LlBlock;
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I64};

/// Gen-GC Phase C2 helper: emit a write barrier after heap-store sites
/// when `PERRY_WRITE_BARRIERS=1`. Sites with a precise field/element
/// address use `js_write_barrier_slot`; opaque helper stores keep using
/// the compatibility wrapper, which conservatively marks the parent span.
/// The env gate is read once and OnceLock-cached at codegen time.
pub(crate) fn emit_write_barrier(ctx: &mut FnCtx<'_>, parent_bits: &str, child_bits: &str) {
    if !crate::codegen::write_barriers_enabled() {
        return;
    }
    ctx.block()
        .call_void("js_write_barrier", &[(I64, parent_bits), (I64, child_bits)]);
}

pub(crate) fn emit_write_barrier_slot_on_block(
    blk: &mut LlBlock,
    parent_bits: &str,
    slot_addr: &str,
    child_bits: &str,
) {
    if !crate::codegen::write_barriers_enabled() {
        return;
    }
    blk.call_void(
        "js_write_barrier_slot",
        &[(I64, parent_bits), (I64, slot_addr), (I64, child_bits)],
    );
}

/// Issue #562 — `super({ ... })` for `class X extends ReadableStream`,
/// `WritableStream`, or `TransformStream`. Extracts the underlying
/// source/sink/transformer callbacks from the inline object literal,
/// lowers each one (TAG_UNDEFINED for missing fields), and calls the
/// runtime `*_subclass_init` shim — which allocates the stream registry
/// handle and stashes it on `this` under `__perry_stream_handle__`.
///
/// `kind` is one of `"readable"` / `"writable"` / `"transform"` —
/// matches the SuperCall arm's `parent_name` switch in expr.rs.
pub(crate) fn lower_stream_super_init(
    ctx: &mut FnCtx<'_>,
    kind: &str,
    super_args: &[Expr],
) -> Result<String> {
    let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));

    // Pre-extract field exprs so we don't hold a borrow across `lower_expr`.
    let opts_props: Option<Vec<(String, Expr)>> = super_args
        .first()
        .and_then(|first| crate::lower_call::extract_options_fields(ctx, first));
    let qstrat_props: Option<Vec<(String, Expr)>> = super_args
        .get(1)
        .and_then(|second| crate::lower_call::extract_options_fields(ctx, second));

    // Lower the canonical callback set per stream kind. Fields not
    // present (or callable arg shape that isn't an inline literal) fall
    // back to TAG_UNDEFINED — matches the existing `new ReadableStream
    // / WritableStream / TransformStream` lowerings in
    // `lower_call/builtin.rs`.
    let mut start = undef_lit.clone();
    let mut pull = undef_lit.clone();
    let mut cancel = undef_lit.clone();
    let mut write = undef_lit.clone();
    let mut close = undef_lit.clone();
    let mut abort = undef_lit.clone();
    let mut transform = undef_lit.clone();
    let mut flush = undef_lit.clone();

    if let Some(props) = opts_props {
        for (k, vexpr) in &props {
            match (kind, k.as_str()) {
                ("readable", "start") => start = lower_expr(ctx, vexpr)?,
                ("readable", "pull") => pull = lower_expr(ctx, vexpr)?,
                ("readable", "cancel") => cancel = lower_expr(ctx, vexpr)?,
                ("writable", "write") => write = lower_expr(ctx, vexpr)?,
                ("writable", "close") => close = lower_expr(ctx, vexpr)?,
                ("writable", "abort") => abort = lower_expr(ctx, vexpr)?,
                ("transform", "transform") => transform = lower_expr(ctx, vexpr)?,
                ("transform", "flush") => flush = lower_expr(ctx, vexpr)?,
                _ => {
                    // Lower for side effects (closure-capture collection,
                    // string-pool registration, etc.) but discard the value.
                    let _ = lower_expr(ctx, vexpr)?;
                }
            }
        }
    } else if let Some(first) = super_args.first() {
        // Caller passed something that isn't a recognized shape — lower
        // for side effects so closure analysis stays consistent.
        let _ = lower_expr(ctx, first)?;
    }

    let mut hwm = double_literal(1.0);
    if let Some(qprops) = qstrat_props {
        for (k, vexpr) in &qprops {
            if k == "highWaterMark" {
                hwm = lower_expr(ctx, vexpr)?;
            } else {
                let _ = lower_expr(ctx, vexpr)?;
            }
        }
    } else if let Some(second) = super_args.get(1) {
        let _ = lower_expr(ctx, second)?;
    }

    // `this` (NaN-boxed pointer) — the runtime shim stashes the handle
    // on it via `js_object_set_field_by_name`.
    let this_slot = ctx.this_stack.last().cloned();
    let this_box = match this_slot {
        Some(slot) => ctx.block().load(DOUBLE, &slot),
        None => undef_lit.clone(),
    };

    let runtime_fn = match kind {
        "readable" => "js_readable_stream_subclass_init",
        "writable" => "js_writable_stream_subclass_init",
        "transform" => "js_transform_stream_subclass_init",
        _ => unreachable!("lower_stream_super_init: unexpected kind {}", kind),
    };

    let blk = ctx.block();
    match kind {
        "readable" => {
            blk.call(
                DOUBLE,
                runtime_fn,
                &[
                    (DOUBLE, &this_box),
                    (DOUBLE, &start),
                    (DOUBLE, &pull),
                    (DOUBLE, &cancel),
                    (DOUBLE, &hwm),
                ],
            );
        }
        "writable" => {
            blk.call(
                DOUBLE,
                runtime_fn,
                &[
                    (DOUBLE, &this_box),
                    (DOUBLE, &write),
                    (DOUBLE, &close),
                    (DOUBLE, &abort),
                    (DOUBLE, &hwm),
                ],
            );
        }
        "transform" => {
            blk.call(
                DOUBLE,
                runtime_fn,
                &[
                    (DOUBLE, &this_box),
                    (DOUBLE, &transform),
                    (DOUBLE, &flush),
                    (DOUBLE, &hwm),
                ],
            );
        }
        _ => unreachable!(),
    }

    Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
}
