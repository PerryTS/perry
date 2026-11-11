//! Record-array intrinsics (DESIGN-p4l4 §1.4, §5.3, VR-4b).
//!
//! A record array is a `GC_TYPE_RECORD_ARRAY`: a head with its length, its
//! capacity and the record's word count, and one edge to a pointer-free
//! buffer that holds the records' words inline (§1.2: every word is a valid
//! JSValue). Nothing but these intrinsics touches one: the `value_records`
//! rewrite turns every operation the facts admit on a record array into one
//! of them, and they are runtime calls (`perry-runtime` `record_array`),
//! each of which checks the head's type:
//!
//! - `new(tmpl)`: the empty array of the template's word count;
//! - `push(a, tmpl, copy)`: the lanes of the copy the rewrite wrote
//!   (`record_sig::copy_lanes`) are stored to words of this frame and copied
//!   into the buffer, which grows behind the head (the head never moves by
//!   growth, so no holder is written back);
//! - `pop(a, tmpl)`, `get(a, tmpl, i)`: a record result as a typed call's,
//!   the presence word returned and the lanes written to words of this
//!   frame (`record_sig::Called::Record`), absent for an empty array or an
//!   index out of bounds; they reach lowering only as the value of an
//!   aggregate temporary (`record_sig::try_lower_agg_set`);
//! - `length(a)`.
//!
//! The words a record crosses in are an entry-block slot of the caller, so
//! no pointer into a buffer is ever held across a call (§6.2).
use anyhow::{bail, Result};
use perry_hir::Expr;

use super::record_sig::Called;
use super::FactsRecRep;
use crate::expr::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I32, I64, PTR};
use crate::value_records::{RA_GET, RA_LENGTH, RA_NEW, RA_POP, RA_PUSH};

/// The record template an intrinsic names, and its word count.
fn layout(ctx: &FnCtx<'_>, method: &str, e: Option<&Expr>) -> Result<(u32, usize, u32)> {
    let Some(Expr::Integer(t)) = e else {
        bail!("ICE: value record array: {method} names no record template");
    };
    let tmpl = *t as u32;
    let Some(l) = ctx.wpfacts.record_layouts.get(&tmpl) else {
        bail!("ICE: value record array: {method} of record template {tmpl}, which this module has no layout for");
    };
    Ok((tmpl, l.keys.len(), l.class_id.unwrap_or(0)))
}

/// `new`, `push` or `length` in a value position: its value.
pub(crate) fn try_lower_value(
    ctx: &mut FnCtx<'_>,
    method: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    match method {
        RA_NEW => {
            let (_, words, shape) = layout(ctx, method, args.first())?;
            let v = ctx.block().call(
                DOUBLE,
                "js_record_array_new",
                &[(I32, &words.to_string()), (I32, &shape.to_string())],
            );
            Ok(Some(v))
        }
        RA_LENGTH => {
            let [a] = args else {
                bail!("ICE: value record array: length takes the array");
            };
            let a = lower_expr(ctx, a)?;
            Ok(Some(ctx.block().call(
                DOUBLE,
                "js_record_array_length",
                &[(DOUBLE, &a)],
            )))
        }
        RA_PUSH => {
            let [a, _, copy] = args else {
                bail!("ICE: value record array: push takes the array, the template and the copy");
            };
            let (tmpl, words, _) = layout(ctx, method, args.get(1))?;
            let facts = ctx.wpfacts.clone();
            // The array is held across the record's evaluation.
            let mut roots = crate::rooting::open_rooted_group(1);
            let idx = roots.lower(ctx, a, true)?;
            let (first, copy) = match copy {
                Expr::Sequence(list) if !list.is_empty() => {
                    let (last, first) = list.split_last().expect("a non-empty sequence");
                    (first, last)
                }
                e => (&[][..], e),
            };
            for x in first {
                lower_expr(ctx, x)?;
            }
            let Some(lanes) = super::record_sig::copy_lanes(&facts, tmpl, copy) else {
                bail!("ICE: value record array: the pushed value is no copy of a record's lanes");
            };
            let buf = ctx.func.alloca_entry_array(DOUBLE, words.max(1));
            for (j, id) in lanes.fields.iter().enumerate() {
                let v = lower_expr(ctx, &Expr::LocalGet(*id))?;
                let blk = ctx.block();
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &j.to_string())]);
                blk.store(DOUBLE, &v, &slot);
            }
            let a = roots.reread(ctx, idx)?;
            let v = ctx
                .block()
                .call(DOUBLE, "js_record_array_push", &[(DOUBLE, &a), (PTR, &buf)]);
            roots.release(ctx);
            Ok(Some(v))
        }
        RA_POP | RA_GET => {
            bail!("ICE: value record array: a record result of {method} is not bound to its lanes")
        }
        _ => Ok(None),
    }
}

/// `pop` or `get` as the value of an aggregate temporary: the presence word
/// and the result words, as a record `$sig` returns them.
pub(crate) fn try_lower_source(ctx: &mut FnCtx<'_>, value: &Expr) -> Result<Option<Called>> {
    let Expr::NativeMethodCall {
        module,
        method,
        object: None,
        args,
        ..
    } = value
    else {
        return Ok(None);
    };
    if module != "__perry_runtime" || !matches!(method.as_str(), RA_POP | RA_GET) {
        return Ok(None);
    }
    let (tmpl, words, _) = layout(ctx, method, args.get(1))?;
    let buf = ctx.func.alloca_entry_array(DOUBLE, words.max(1));
    let present = match (method.as_str(), args.as_slice()) {
        (RA_POP, [a, _]) => {
            let a = lower_expr(ctx, a)?;
            ctx.block()
                .call(DOUBLE, "js_record_array_pop", &[(DOUBLE, &a), (PTR, &buf)])
        }
        (RA_GET, [a, _, i]) => {
            // The array is held across the index's evaluation.
            let mut roots = crate::rooting::open_rooted_group(1);
            let collects = crate::rooting::any_operand_may_collect(ctx, std::iter::once(i));
            let idx = roots.lower(ctx, a, collects)?;
            let i = lower_expr(ctx, i)?;
            let a = roots.reread(ctx, idx)?;
            let v = ctx.block().call(
                DOUBLE,
                "js_record_array_get",
                &[(DOUBLE, &a), (DOUBLE, &i), (PTR, &buf)],
            );
            roots.release(ctx);
            v
        }
        _ => bail!("ICE: value record array: malformed {method}"),
    };
    Ok(Some(Called::Record {
        present,
        words: buf,
        rep: FactsRecRep { tmpl, opt: true },
    }))
}
