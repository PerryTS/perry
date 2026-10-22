//! Chunked literal births (`codegen::split_regions` literal chunks).
//!
//! A `New` of a literal-shape class whose trailing arguments are
//! [`Expr::LiteralChunks`] is born exactly as before, by the same lowering
//! with the same original arguments: a chunked argument lowers to the birth's
//! own `undefined` fill. The born object is then rooted (the ordinary
//! accumulator root) and handed, as argument 0, to each chunk function in
//! source order; it is the expression's value only after the last call. A
//! chunk function's body initializes its slots with [`lower_slot_init`].

use anyhow::{anyhow, bail, ensure, Result};
use perry_hir::Expr;

use super::{lower_expr, FnCtx};
use crate::nanbox::POINTER_MASK_I64;
use crate::rooting::{self, Arg, Repr};
use crate::types::{DOUBLE, I32, I64};

/// The chunk calls of a chunked birth's arguments, if `args` is one.
pub(crate) fn chunk_calls(args: &[Expr]) -> Option<Result<&[Expr]>> {
    let at = args
        .iter()
        .position(|a| matches!(a, Expr::LiteralChunks(_)))?;
    let Expr::LiteralChunks(calls) = &args[at] else {
        unreachable!()
    };
    if !args[at + 1..]
        .iter()
        .all(|a| matches!(a, Expr::LiteralChunks(c) if c.is_empty()))
    {
        return Some(Err(anyhow!(
            "literal chunks: the chunked arguments are not one suffix"
        )));
    }
    Some(Ok(calls))
}

/// Birth with `birth` (the ordinary lowering of the same `New`), then run the
/// chunk calls on the rooted object.
pub(crate) fn lower_chunked_birth<'f>(
    ctx: &mut FnCtx<'f>,
    calls: &[Expr],
    birth: impl FnOnce(&mut FnCtx<'f>) -> Result<String>,
) -> Result<String> {
    let outer = std::mem::replace(&mut ctx.literal_chunk_fill, true);
    let born = birth(ctx);
    ctx.literal_chunk_fill = outer;
    let born = born?;
    rooting::with_rooted_accumulator(
        ctx,
        Repr::Boxed,
        &born,
        true,
        |ctx, object| {
            for call in calls {
                let Expr::Call { callee, args, .. } = call else {
                    bail!("literal chunks: a chunk is not a call");
                };
                let Expr::FuncRef(fid) = callee.as_ref() else {
                    bail!("literal chunks: a chunk is not a direct call");
                };
                let fname =
                    ctx.func_names.get(fid).cloned().ok_or_else(|| {
                        anyhow!("literal chunks: chunk function {fid} has no symbol")
                    })?;
                let [Expr::LiteralChunkObject, live_ins @ ..] = args.as_slice() else {
                    bail!("literal chunks: a chunk does not take the object first");
                };
                // The other arguments are by-value live-ins, plain local reads:
                // lowering them cannot collect, so they are plain operands of
                // the call that re-reads the object.
                let mut values = Vec::with_capacity(live_ins.len());
                for a in live_ins {
                    ensure!(
                        matches!(a, Expr::LocalGet(_)),
                        "literal chunks: a live-in is not a local read"
                    );
                    values.push(lower_expr(ctx, a)?);
                }
                let rest: Vec<Arg<'_>> = values.iter().map(|v| Arg::Plain(DOUBLE, v)).collect();
                object.call(ctx, DOUBLE, &fname, &rest);
            }
            Ok(())
        },
        |_, object| Ok(object.to_string()),
    )
}

/// A constructor-prologue store of a chunked slot is skipped: the slot keeps
/// the allocation's fill until its chunk function stores the value.
pub(crate) fn is_fill(arg: Option<&Expr>) -> bool {
    matches!(arg, Some(Expr::LiteralChunks(_)))
}

/// A chunked argument of the birth being lowered: the birth's `undefined`
/// fill (a chunk stores the slot's value after the birth).
pub(crate) fn lower_fill(ctx: &FnCtx<'_>) -> Result<String> {
    ensure!(
        ctx.literal_chunk_fill,
        "literal chunks: a chunked argument outside its birth"
    );
    Ok(crate::nanbox::double_literal(f64::from_bits(
        crate::nanbox::TAG_UNDEFINED,
    )))
}

/// `LiteralSlotInit`: evaluate the value, then store it into the slot of the
/// unpublished birth the chunk's object parameter holds. The object is read
/// after the value, so it is the post-collection address; the store is the
/// runtime's indexed slot store (representation check and write barrier).
pub(crate) fn lower_slot_init(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    slot: u32,
    value: &Expr,
) -> Result<String> {
    ensure!(
        matches!(object, Expr::LocalGet(_)),
        "literal chunks: a slot init's object is not the chunk parameter"
    );
    let v = lower_expr(ctx, value)?;
    let o = lower_expr(ctx, object)?;
    let blk = ctx.block();
    let o_bits = blk.bitcast_double_to_i64(&o);
    let handle = blk.and(I64, &o_bits, POINTER_MASK_I64);
    let v_bits = blk.bitcast_double_to_i64(&v);
    blk.call_void(
        "js_object_set_field",
        &[(I64, &handle), (I32, &slot.to_string()), (I64, &v_bits)],
    );
    Ok(crate::nanbox::double_literal(f64::from_bits(
        crate::nanbox::TAG_UNDEFINED,
    )))
}
