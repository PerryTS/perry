//! Compact source-level `a[a.length] = v`. General stores keep their overwrite
//! guard; append work that cannot be a raw store is shared in the runtime.
use anyhow::Result;
use perry_hir::Expr;

use super::FnCtx;
use crate::types::{DOUBLE, I1, I16, I32, I64, I8};

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
            let named_idx = super::array_proto_guard::program_writes_element_descriptors()
                .then(|| ctx.new_block("append.named"));
            let admitted_header = named_idx
                .map(|idx| ctx.block_label(idx))
                .unwrap_or_else(|| room.clone());
            ctx.current_block = header_idx;
            let word = {
                let blk = ctx.block();
                // Forwarding, integrity, descriptors, custom prototype,
                // element shape, and layout are one word admission.
                let word = super::index_get::guarded_array::emit_array_guard_word(blk, &handle);
                let shape = if scalar {
                    // Canonical numbers also admit raw-f64 slots. The scan
                    // declaration may still change (ALL_POINTERS -> UNKNOWN).
                    let mask = "407339263"; // 0x1847_80FF
                    let expected = "1"; // ARRAY
                    let masked = blk.and(I32, &word, mask);
                    blk.icmp_eq(I32, &masked, expected)
                } else {
                    // Plain slots may change their GC layout in O(1). Keep
                    // raw-f64 conversion and homogeneous element-shape work
                    // in the runtime, but do not reject the first pointer in
                    // POINTER_FREE or a scalar in ALL_POINTERS.
                    let masked = blk.and(I32, &word, "415727871"); // 0x18C7_80FF
                    blk.icmp_eq(I32, &masked, "1")
                };
                blk.cond_br(&shape, &admitted_header, &slow);
                word
            };
            if let Some(idx) = named_idx {
                // The word admitted a real array before reading its payload.
                // Null named layout proves descriptor absence in O(1); named
                // layouts retain the complete Set continuation.
                ctx.current_block = idx;
                let blk = ctx.block();
                let named = super::array_proto_guard::emit_array_has_named_layout(blk, &handle);
                let plain = blk.xor(I1, &named, "true");
                blk.cond_br(&plain, &room, &slow);
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
                    let payload = blk.and(I64, &value_bits, crate::nanbox::POINTER_MASK_I64);
                    let heap = blk.icmp_uge(I64, &payload, "4096");
                    let heap_string = blk.and(I1, &is_string, &heap);
                    blk.cond_br(&heap_string, &string, &fast);
                }
                ctx.current_block = string_idx;
                let share_idx = ctx.new_block("append.share");
                let share = ctx.block_label(share_idx);
                let refcount_ptr = {
                    let blk = ctx.block();
                    let value_bits = blk.bitcast_double_to_i64(stored);
                    let payload = blk.and(I64, &value_bits, crate::nanbox::POINTER_MASK_I64);
                    let string_ptr = blk.inttoptr(I64, &payload);
                    // Shared image strings are read-only. Only a unique
                    // heap string needs the runtime addref's demotion.
                    let ptr = blk.gep(I8, &string_ptr, &[(I64, "12")]);
                    let refcount = blk.load(I32, &ptr);
                    let unique = blk.icmp_ne(I32, &refcount, "0");
                    blk.cond_br(&unique, &share, &fast);
                    ptr
                };
                ctx.current_block = share_idx;
                {
                    let blk = ctx.block();
                    // GC_STORE_AUDIT(POINTER_FREE): string ownership hint.
                    blk.store(I32, "0", &refcount_ptr);
                    blk.br(&fast);
                }
            }
            ctx.current_block = fast_idx;
            super::store_census::bump(ctx, super::store_census::ELEM_STORE_APPEND);
            if scalar {
                // A construction-proven number cannot carry a child. Keep a
                // pointer-free declaration; every other precise scan kind
                // becomes UNKNOWN without a runtime layout note.
                let blk = ctx.block();
                let flags32 = blk.lshr(I32, &word, "16");
                let flags = blk.trunc(I32, &flags32, I16);
                let layout = blk.and(I16, &flags, "57344");
                let free = blk.icmp_eq(I16, &layout, "16384");
                let unknown = blk.and(I16, &flags, "8191");
                let next = blk.select(I1, &free, I16, &flags, &unknown);
                let addr = blk.sub(I64, &handle, "6");
                let ptr = blk.inttoptr(I64, &addr);
                // GC_STORE_AUDIT(POINTER_FREE): scalar scan declaration.
                blk.store(I16, &next, &ptr);
            } else {
                emit_plain_layout_transition(ctx, &handle, &word, &length, stored);
            }
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
                // Admitted only without a named layout: no element
                // descriptor at the frontier (EK §3.1).
                ctx.note_ek_frontier(&handle, &length, &value_bits);
                ctx.block().br(&done);
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

/// Update the GC scan declaration for an append to plain boxed slots. The
/// guard excluded raw-f64/holes and element-shape metadata. This is the same
/// state transition as `layout_note_slot`, before the slot and length stores;
/// it never allocates, calls user code, or discards an existing pointer edge.
fn emit_plain_layout_transition(
    ctx: &mut FnCtx<'_>,
    handle: &str,
    word: &str,
    length: &str,
    stored: &str,
) {
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(stored);
    let top = blk.lshr(I64, &bits, "48");
    let object = blk.icmp_eq(I64, &top, crate::nanbox::POINTER_TAG_TOP16_I64);
    let string = blk.icmp_eq(I64, &top, crate::nanbox::STRING_TAG_TOP16_I64);
    let bigint = blk.icmp_eq(I64, &top, crate::nanbox::BIGINT_TAG_TOP16_I64);
    let tagged = blk.or(I1, &object, &string);
    let tagged = blk.or(I1, &tagged, &bigint);
    let payload = blk.and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let nonnull = blk.icmp_ne(I64, &payload, "0");
    let tagged_pointer = blk.and(I1, &tagged, &nonnull);
    // Match `layout_pointer_bearing_bits`, including its legacy aligned raw
    // address band. No annotation can justify a POINTER_FREE declaration.
    let raw_offset = blk.sub(I64, &bits, "4096");
    let raw_range = blk.icmp_ult(I64, &raw_offset, "281474976706560");
    let low = blk.and(I64, &bits, "7");
    let aligned = blk.icmp_eq(I64, &low, "0");
    let raw_pointer = blk.and(I1, &raw_range, &aligned);
    let pointer = blk.or(I1, &tagged_pointer, &raw_pointer);
    let scalar = blk.xor(I1, &pointer, "true");

    let flags32 = blk.lshr(I32, word, "16");
    let flags = blk.trunc(I32, &flags32, I16);
    let layout = blk.and(I16, &flags, "57344"); // 0xE000
    let was_free = blk.icmp_eq(I16, &layout, "16384"); // POINTER_FREE
    let was_all = blk.icmp_eq(I16, &layout, "40960"); // SIDE_MASK | ALL_POINTERS
    let keep_free = blk.and(I1, &was_free, &scalar);
    let keep_all = blk.and(I1, &was_all, &pointer);
    let empty = blk.icmp_eq(I32, length, "0");
    let first = blk.and(I1, &was_free, &empty);
    let first_pointer = blk.and(I1, &first, &pointer);
    let all = blk.or(I1, &keep_all, &first_pointer);
    let free_or_unknown = blk.select(I1, &keep_free, I16, "16384", "0");
    let next_layout = blk.select(I1, &all, I16, "40960", &free_or_unknown);
    let other_flags = blk.and(I16, &flags, "8191"); // preserve every non-layout bit
    let next_flags = blk.or(I16, &other_flags, &next_layout);
    let flags_addr = blk.sub(I64, handle, "6");
    let flags_ptr = blk.inttoptr(I64, &flags_addr);
    // GC_STORE_AUDIT(POINTER_FREE): GC scan declaration. The slot barrier and
    // length publication remain after the initialized element store.
    blk.store(I16, &next_flags, &flags_ptr);
}
