//! Virtual suffix lowering. Only offsets are stored outside ordinary locals;
//! the existing source-local root remains responsible for GC relocation.

use crate::expr::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I1, I32, I64};
use anyhow::Result;
use perry_hir::Expr;

pub(crate) fn initialize(ctx: &mut FnCtx<'_>, id: u32) {
    let slot = ctx.func.alloca_entry("[3 x i32]");
    ctx.block().store("[3 x i32]", "zeroinitializer", &slot);
    ctx.suffix_cursors.insert(id, slot);
}

pub(crate) fn try_lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    let (id, operation, index) = match expr {
        Expr::PropertyGet {
            object, property, ..
        } if property == "length" => {
            let Expr::LocalGet(id) = object.as_ref() else {
                return Ok(None);
            };
            (*id, "length", 0)
        }
        Expr::LocalSet(id, value) => {
            let Some((receiver, count)) = crate::collectors::suffix_strings::method(value, "slice")
            else {
                return Ok(None);
            };
            if *id != receiver || count < 0 {
                return Ok(None);
            }
            (*id, "advance", count)
        }
        _ => {
            let Some((id, index)) = crate::collectors::suffix_strings::method(expr, "charCodeAt")
            else {
                return Ok(None);
            };
            (id, "char_code_at", index)
        }
    };
    let Some(cursor_slot) = ctx.suffix_cursors.get(&id).cloned() else {
        return Ok(None);
    };
    let source = lower_expr(ctx, &Expr::LocalGet(id))?;
    let bits = ctx.block().bitcast_double_to_i64(&source);
    let tag = ctx.block().lshr(I64, &bits, "48");
    let heap = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::STRING_TAG_TOP16_I64);
    let short = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::SHORT_STRING_TAG_TOP16_I64);
    let is_string = ctx.block().or(I1, &heap, &short);
    let fast = ctx.new_block("suffix.string");
    let slow = ctx.new_block("suffix.other");
    let merge = ctx.new_block("suffix.merge");
    let fast_label = ctx.block_label(fast);
    let slow_label = ctx.block_label(slow);
    let merge_label = ctx.block_label(merge);
    ctx.block().cond_br(&is_string, &fast_label, &slow_label);
    ctx.current_block = fast;
    let cursor = ctx.block().ptrtoint(&cursor_slot, I64);
    let index = index.to_string();
    let fast_value = if operation == "advance" {
        ctx.block().call_void(
            "js_string_suffix_advance",
            &[(DOUBLE, &source), (I64, &cursor), (I32, &index)],
        );
        source
    } else {
        let mut args = vec![(DOUBLE, source.as_str()), (I64, cursor.as_str())];
        if operation == "char_code_at" {
            args.push((I32, &index));
        }
        ctx.block()
            .call(DOUBLE, &format!("js_string_suffix_{operation}"), &args)
    };
    let fast_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = slow;
    // Types are not runtime validation. Preserve the existing method/property
    // behavior if a string-annotated local actually contains another value.
    ctx.suffix_cursors.remove(&id);
    let fallback = lower_expr(ctx, expr);
    ctx.suffix_cursors.insert(id, cursor_slot);
    let fallback = fallback?;
    let slow_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);
    ctx.current_block = merge;
    Ok(Some(ctx.block().phi(
        DOUBLE,
        &[(&fast_value, &fast_pred), (&fallback, &slow_pred)],
    )))
}
