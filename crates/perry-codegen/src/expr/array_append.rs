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
                // element shape, and layout are one word admission.
                let word = super::index_get::guarded_array::emit_array_guard_word(blk, &handle);
                let shape = if scalar {
                    // Numbers preserve POINTER_FREE.
                    let mask = "4165435647"; // 0xF847_80FF
                    let expected = "1073741825"; // POINTER_FREE | ARRAY
                    let masked = blk.and(I32, &word, mask);
                    blk.icmp_eq(I32, &masked, expected)
                } else {
                    // Any value: exactly the stores `js_array_append_set`
                    // makes without a layout note (`note_noop`): a plain
                    // element kind (no raw-f64 layout, raw holes or element
                    // shape) whose layout the value leaves as it is —
                    // UNKNOWN for any value, POINTER_FREE for a value that
                    // carries no pointer, ALL_POINTERS for one that does.
                    let mask = "4173824255"; // 0xF8C7_80FF
                    let masked = blk.and(I32, &word, mask);
                    let unknown = blk.icmp_eq(I32, &masked, "1");
                    let pointer_free = blk.icmp_eq(I32, &masked, "1073741825"); // 0x4000_0001
                    let all_pointers = blk.icmp_eq(I32, &masked, "2684354561"); // 0xA000_0001
                    let value_bits = blk.bitcast_double_to_i64(stored);
                    let top16 = blk.lshr(I64, &value_bits, "48");
                    let is_object = blk.icmp_eq(I64, &top16, crate::nanbox::POINTER_TAG_TOP16_I64);
                    let is_string = blk.icmp_eq(I64, &top16, crate::nanbox::STRING_TAG_TOP16_I64);
                    let is_bigint = blk.icmp_eq(I64, &top16, crate::nanbox::BIGINT_TAG_TOP16_I64);
                    let pointer_tag = blk.or(crate::types::I1, &is_object, &is_string);
                    let pointer_tag = blk.or(crate::types::I1, &pointer_tag, &is_bigint);
                    let payload = blk.and(I64, &value_bits, crate::nanbox::POINTER_MASK_I64);
                    let payload_heap = blk.icmp_uge(I64, &payload, "4096");
                    // `layout_pointer_bearing_bits`: a pointer tag with a
                    // payload. Admitted conservatively: a pointer payload
                    // below 4096, and an untagged double, take the runtime.
                    let pointer = blk.and(crate::types::I1, &pointer_tag, &payload_heap);
                    let boxed = blk.icmp_uge(I64, &top16, "32760"); // 0x7FF8
                    let not_pointer_tag = blk.xor(crate::types::I1, &pointer_tag, "true");
                    let no_pointer = blk.and(crate::types::I1, &boxed, &not_pointer_tag);
                    let free_ok = blk.and(crate::types::I1, &pointer_free, &no_pointer);
                    let all_ok = blk.and(crate::types::I1, &all_pointers, &pointer);
                    let ok = blk.or(crate::types::I1, &unknown, &free_ok);
                    let ok = blk.or(crate::types::I1, &ok, &all_ok);
                    // A heap string must be shared before a second holder
                    // sees it (`js_string_addref_if_heap_string`): one whose
                    // refcount is still unique takes the runtime.
                    let low_string = blk.xor(crate::types::I1, &payload_heap, "true");
                    let low_string = blk.and(crate::types::I1, &is_string, &low_string);
                    let no_low_string = blk.xor(crate::types::I1, &low_string, "true");
                    blk.and(crate::types::I1, &ok, &no_low_string)
                };
                // Element descriptors and an own `push` live in the
                // array's named layout.
                let shape =
                    super::index_get::guarded_array::emit_and_no_named_layout(blk, &shape, &handle);
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
                (length, admitted)
            };
            let (length, admitted) = length;
            if scalar {
                ctx.block().cond_br(&admitted, &fast, &slow);
            } else {
                // A heap string's refcount is read only once the header test
                // admitted it (payload >= 4096).
                let pick_idx = ctx.new_block("append.pick");
                let string_idx = ctx.new_block("append.string");
                let pick = ctx.block_label(pick_idx);
                let string = ctx.block_label(string_idx);
                ctx.block().cond_br(&admitted, &pick, &slow);
                ctx.current_block = pick_idx;
                {
                    let blk = ctx.block();
                    let value_bits = blk.bitcast_double_to_i64(stored);
                    let top16 = blk.lshr(I64, &value_bits, "48");
                    let is_string = blk.icmp_eq(I64, &top16, crate::nanbox::STRING_TAG_TOP16_I64);
                    blk.cond_br(&is_string, &string, &fast);
                }
                ctx.current_block = string_idx;
                {
                    let blk = ctx.block();
                    let value_bits = blk.bitcast_double_to_i64(stored);
                    let payload = blk.and(I64, &value_bits, crate::nanbox::POINTER_MASK_I64);
                    let string_ptr = blk.inttoptr(I64, &payload);
                    // `StringHeader::refcount` (offset 12): 0 = shared, the
                    // state `js_string_addref_if_heap_string` would leave.
                    let refcount_ptr = blk.gep(I8, &string_ptr, &[(I64, "12")]);
                    let refcount = blk.load(I32, &refcount_ptr);
                    let shared = blk.icmp_eq(I32, &refcount, "0");
                    blk.cond_br(&shared, &fast, &slow);
                }
            }
            ctx.current_block = fast_idx;
            super::store_census::bump(ctx, super::store_census::ELEM_STORE_APPEND);
            {
                let (slot_addr, value_bits) = {
                    let blk = ctx.block();
                    let elements = blk.array_elements_addr(&handle);
                    let length64 = blk.zext(I32, &length, I64);
                    let offset = blk.shl(I64, &length64, "3");
                    let slot_addr = blk.add(I64, &elements, &offset);
                    let slot = blk.inttoptr(I64, &slot_addr);
                    // GC_STORE_AUDIT(BARRIERED): a number into a header-proven
                    // pointer-free array needs no barrier; any other value is
                    // barriered below, before the length publishes the slot.
                    blk.store(DOUBLE, stored, &slot);
                    let value_bits = blk.bitcast_double_to_i64(stored);
                    (slot_addr, value_bits)
                };
                if !scalar {
                    let receiver_bits = ctx.block().bitcast_double_to_i64(receiver);
                    crate::expr::write_barrier::emit_write_barrier_slot_value_and_generation_tested(
                        ctx,
                        &handle,
                        &receiver_bits,
                        &slot_addr,
                        &value_bits,
                        "append",
                    );
                }
                let blk = ctx.block();
                let next = blk.add(I32, &length, "1");
                let arr = blk.inttoptr(I64, &handle);
                // GC_STORE_AUDIT(POINTER_FREE): publish the initialized slot.
                blk.store(I32, &next, &arr);
                blk.br(&done);
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
