//! Compact source-level `a[a.length] = v`. General stores keep their overwrite
//! guard; append work that cannot be a raw store is shared in the runtime.
use anyhow::Result;
use perry_hir::Expr;

use super::FnCtx;
use crate::types::{DOUBLE, I32, I64, I8};

#[cfg(test)]
#[path = "array_append_tests.rs"]
mod tests;

pub(super) fn try_lower(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    value: &Expr,
    strict: bool,
) -> Result<Option<String>> {
    // Only a repeated local has an effect-free receiver identity. Property
    // receivers may run a getter twice and must keep the ordinary Set path.
    let Expr::LocalGet(id) = object else {
        return Ok(None);
    };
    let Expr::PropertyGet {
        object: length_owner,
        property,
        ..
    } = index
    else {
        return Ok(None);
    };
    if property != "length"
        || !matches!(length_owner.as_ref(), Expr::LocalGet(other) if other == id)
        || super::typed_feedback_emission_enabled()
    {
        return Ok(None);
    }
    // Types are hints. Only construction proofs let the raw store omit
    // kind conversion/string ownership and layout bookkeeping.
    let scalar = crate::type_analysis::expr_produces_canonical_raw_f64(ctx, value);
    let home = if !ctx.boxed_vars.contains(id)
        && !ctx.closure_captures.contains_key(id)
        && !ctx.module_globals.contains_key(id)
    {
        ctx.locals.get(id).cloned()
    } else {
        None
    };
    let result =
        crate::rooting::with_operands_rooted(ctx, &[object, index, value], |ctx, vals| {
            let (receiver, key, stored) = (&vals[0], &vals[1], &vals[2]);
            if ctx.block().is_terminated() {
                return Ok(stored.clone());
            }
            let slow_idx = ctx.new_block("append.slow");
            let done_idx = ctx.new_block("append.done");
            let slow = ctx.block_label(slow_idx);
            let done = ctx.block_label(done_idx);
            if scalar {
                let header_idx = ctx.new_block("append.header");
                let room_idx = ctx.new_block("append.room");
                let fast_idx = ctx.new_block("append.fast");
                let header = ctx.block_label(header_idx);
                let room = ctx.block_label(room_idx);
                let fast = ctx.block_label(fast_idx);
                let handle = {
                    let blk = ctx.block();
                    let bits = blk.bitcast_double_to_i64(receiver);
                    let offset = blk.sub(
                        I64,
                        &bits,
                        super::index_get::guarded_array::HEAP_POINTER_BAND_BASE_I64,
                    );
                    let valid = blk.icmp_ult(
                        I64,
                        &offset,
                        super::index_get::guarded_array::HEAP_POINTER_STORE_BAND_SPAN_I64,
                    );
                    let handle = blk.add(
                        I64,
                        &offset,
                        crate::expr::receiver_range::HANDLE_FLOOR_LITERAL,
                    );
                    blk.cond_br(&valid, &header, &slow);
                    handle
                };
                ctx.current_block = header_idx;
                {
                    let blk = ctx.block();
                    // Forwarding, integrity, descriptors, custom prototype,
                    // element shape, and layout are one word admission. Numbers
                    // preserve POINTER_FREE; pointer/unknown values use the shared
                    // continuation so their barrier never adds per-site spills.
                    let word = super::index_get::guarded_array::emit_array_guard_word(blk, &handle);
                    let mask = "4165435647"; // 0xF847_80FF
                    let expected = "1073741825"; // POINTER_FREE | ARRAY
                    let masked = blk.and(I32, &word, mask);
                    let shape = blk.icmp_eq(I32, &masked, expected);
                    // Element descriptors and an own `push` live in the
                    // array's named layout.
                    let shape = super::index_get::guarded_array::emit_and_no_named_layout(
                        blk, &shape, &handle,
                    );
                    blk.cond_br(&shape, &room, &slow);
                }
                ctx.current_block = room_idx;
                let length = {
                    let blk = ctx.block();
                    let arr = blk.inttoptr(I64, &handle);
                    let length = blk.load(I32, &arr);
                    let length_double = blk.uitofp(I32, &length, DOUBLE);
                    let append = blk.fcmp("oeq", key, &length_double);
                    let capacity_ptr = blk.gep(I8, &arr, &[(I64, "4")]);
                    let capacity = blk.load(I32, &capacity_ptr);
                    let room = blk.icmp_ult(I32, &length, &capacity);
                    let clean = blk.load_volatile(I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
                    let clean = blk.and(I8, &clean, super::array_proto_guard::INDEX_INVALIDATED_I8);
                    let clean = blk.icmp_eq(I8, &clean, "0");
                    let admitted = blk.and(crate::types::I1, &append, &room);
                    let admitted = blk.and(crate::types::I1, &admitted, &clean);
                    blk.cond_br(&admitted, &fast, &slow);
                    length
                };
                ctx.current_block = fast_idx;
                super::store_census::bump(ctx, super::store_census::ELEM_STORE_APPEND);
                {
                    let blk = ctx.block();
                    let elements = blk.array_elements_addr(&handle);
                    let length64 = blk.zext(I32, &length, I64);
                    let offset = blk.shl(I64, &length64, "3");
                    let slot_addr = blk.add(I64, &elements, &offset);
                    let slot = blk.inttoptr(I64, &slot_addr);
                    // GC_STORE_AUDIT(POINTER_FREE): construction-proven number
                    // into a header-proven pointer-free array.
                    blk.store(DOUBLE, stored, &slot);
                    let next = blk.add(I32, &length, "1");
                    let arr = blk.inttoptr(I64, &handle);
                    // GC_STORE_AUDIT(POINTER_FREE): publish the initialized slot.
                    blk.store(I32, &next, &arr);
                    blk.br(&done);
                }
            } else {
                ctx.block().br(&slow);
            }
            ctx.current_block = slow_idx;
            // Keep root allocas in the recognized load/store protocol. Their
            // addresses cannot escape through a runtime call under RS4GC.
            let current = home
                .as_ref()
                .map(|slot| ctx.block().load(DOUBLE, slot))
                .unwrap_or_else(|| receiver.clone());
            let selected = ctx.block().call(
                DOUBLE,
                "js_array_append_set",
                &[
                    (DOUBLE, receiver),
                    (DOUBLE, key),
                    (DOUBLE, stored),
                    (I32, if strict { "1" } else { "0" }),
                    (DOUBLE, &current),
                ],
            );
            if let Some(slot) = &home {
                super::emit_root_nanbox_store_on_block(ctx.block(), &selected, slot);
            }
            ctx.block().br(&done);
            ctx.current_block = done_idx;
            Ok(stored.clone())
        })?;
    Ok(Some(result))
}
