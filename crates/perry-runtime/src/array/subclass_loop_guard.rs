//! Object-backed Array-subclass counted-loop admission: the loop guards and
//! the live revalidation entry points generated loops call.
//!
//! Child module of `subclass.rs`, split out to stay under the 2,000-line file
//! gate; `use super::*` keeps the parent's private layout helpers reachable.

use super::*;

/// Admit a complete counted-loop range over either an ordinary Array or an
/// object-backed Array subclass. The seven output words are scalar facts, not
/// managed pointers, so the generated loop can reload a relocated receiver
/// from its root before each residual check.
///
/// Layout: `(kind, gc_header, receiver_header, length_slot|elements, element_base,
/// dense_prefix|inline_bound<<32, bound)`. Kind 1 is an ArrayHeader, kind 2 is
/// a shape-carried ObjectHeader Array subclass, and kind 3 is an
/// elements-backed one (`super::subclass_elements`) whose payload lives in a
/// separate Array: word 3 carries that store's user address, because the
/// generated loop derives its base from the RECEIVER on the non-capture path
/// and the receiver is the object, not the payload. A zero return leaves every
/// semantic case to the unchanged generic loop.
/// Resolve an elements-backed Array-subclass receiver to its inner array
/// (`None` for every other receiver, including the shape-carried subclass
/// form, which keeps the kind-2 admission below).
#[inline]
fn elements_loop_source(
    raw: *const u8,
    header: &'static crate::gc::GcHeader,
) -> Option<(*const u8, &'static crate::gc::GcHeader)> {
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let elements =
        unsafe { crate::array::subclass_elements::elements_of(raw.cast::<ObjectHeader>()) }
            as *const u8;
    if elements.is_null() {
        return None;
    }
    let elements_header =
        unsafe { crate::value::addr_class::try_read_gc_header(elements as usize) }?;
    (elements_header.obj_type == crate::gc::GC_TYPE_ARRAY
        && elements_header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0)
        .then_some((elements, elements_header))
}

/// Admit an elements-backed Array-subclass receiver as a kind-3 loop over its
/// store. Every proof is the store's (an ordinary Array): no descriptors, the
/// prototype latch clear, `bound <= length <= capacity`, and — for a numeric
/// mode — the whole-array raw-f64 bit. Mode 2 (the fused ECS entity-id clone)
/// wants the per-prefix payload a shape-carried subclass publishes, so it is
/// declined here exactly as it is for a plain Array.
fn elements_backed_loop_guard(
    receiver: *const u8,
    elements: *const u8,
    elements_header: &'static crate::gc::GcHeader,
    requested_bound: Option<u32>,
    require_numeric: i32,
    out: *mut u64,
) -> Option<(i32, *const u8)> {
    if elements_header._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
        || super::super::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.load(Ordering::Relaxed) != 0
        || require_numeric >= 2
    {
        return None;
    }
    let array = elements.cast::<ArrayHeader>();
    let (length, capacity) = unsafe { ((*array).length, (*array).capacity) };
    let bound = requested_bound.unwrap_or(length);
    if bound > length || length > capacity || capacity > 16_000_000 {
        return None;
    }
    if require_numeric != 0
        && !unsafe { super::super::header::ensure_array_numeric_raw_f64(array as *mut ArrayHeader) }
    {
        return None;
    }
    let gc_word = unsafe { ptr::read_unaligned(elements.sub(8).cast::<u64>()) };
    let array_word = (u64::from(capacity) << 32) | u64::from(length);
    unsafe {
        out.add(0).write(3);
        out.add(1).write(gc_word);
        out.add(2).write(array_word);
        // Word 3 is the payload address the generated loop reads through; the
        // revalidation entry refreshes it after every iteration that can move
        // or re-allocate the store.
        out.add(3).write(elements as u64);
        out.add(4).write(0);
        out.add(5).write(0);
        out.add(6).write(u64::from(bound));
    }
    // The live address is the RECEIVER: the capture-safe caller reloads the
    // receiver and re-derives the payload from word 3, so handing back the
    // store here would let a stale capture read a detached payload.
    Some((3, receiver))
}

fn packed_arraylike_loop_guard(
    receiver: f64,
    bound: f64,
    require_numeric: i32,
    out: *mut u64,
) -> Option<(i32, *const u8)> {
    let live_length_bound = bound == -1.0;
    if out.is_null()
        || !bound.is_finite()
        || (!live_length_bound && bound < 0.0)
        || (!live_length_bound && bound.fract() != 0.0)
        || bound > 16_000_000.0
    {
        return None;
    }
    let requested_bound = (!live_length_bound).then_some(bound as u32);
    let js = JSValue::from_bits(receiver.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let source = js.as_pointer::<u8>();
    let Some(source_header) =
        (unsafe { crate::value::addr_class::try_read_gc_header(source as usize) })
    else {
        return None;
    };
    // Array growth preserves identity with a forwarding stub. Captured const
    // slots cannot be canonicalized like compiler-private locals, so admit one
    // validated edge and return the live address to codegen. A longer chain,
    // a cross-brand target, or an unreadable target remains a generic-loop
    // side exit. Moving GC normally rewrites closure slots, but accepting the
    // same representation here also makes forced-evacuation entry fail-safe.
    let raw = if source_header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        if source_header.obj_type != crate::gc::GC_TYPE_ARRAY {
            return None;
        }
        let target = unsafe { crate::gc::forwarding_address(source_header) };
        let target_header =
            unsafe { crate::value::addr_class::try_read_gc_header(target as usize) }?;
        if target_header.obj_type != crate::gc::GC_TYPE_ARRAY
            || target_header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
        target
    } else {
        source
    };
    let header = unsafe { crate::value::addr_class::try_read_gc_header(raw as usize) }?;
    // An elements-backed Array-subclass instance keeps its payload in a
    // separate Array hanging off the meta record. It is admitted as its own
    // KIND rather than as a plain Array: kind 1 means "the receiver IS the
    // ArrayHeader" and the generated loop computes `receiver + header + i*8`
    // itself, so answering kind 1 with the store's address made element 0 read
    // the object's `meta` word (`issue_8773_closure_capture_packed_loops`'s
    // dense case). Kind 3 publishes the store address in word 3 and every
    // proof below is the store's.
    if let Some((elements, elements_header)) = elements_loop_source(raw, header) {
        return elements_backed_loop_guard(
            raw,
            elements,
            elements_header,
            requested_bound,
            require_numeric,
            out,
        );
    }

    if header.obj_type == crate::gc::GC_TYPE_ARRAY {
        if header._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
            || super::super::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.load(Ordering::Relaxed) != 0
        {
            return None;
        }
        let array = raw.cast::<ArrayHeader>();
        let (length, capacity) = unsafe { ((*array).length, (*array).capacity) };
        let bound = requested_bound.unwrap_or(length);
        if bound > length || length > capacity || capacity > 16_000_000 {
            return None;
        }
        // Mode 2 is the stronger ECS entity-id contract. Plain Arrays do not
        // carry the per-prefix payload needed to distinguish an exact-u32
        // proof from their whole-array raw-f64 bit, so retain the generic
        // clone for them.
        if require_numeric >= 2 {
            return None;
        }
        if require_numeric != 0 {
            // The raw-f64 invariant is an O(1) GcHeader bit after its first
            // self-healing scan, and every nonnumeric Array write already
            // clears it. Reuse that representation proof instead of walking
            // the full range on every invocation of the surrounding scan().
            if !unsafe {
                super::super::header::ensure_array_numeric_raw_f64(array as *mut ArrayHeader)
            } {
                return None;
            }
        }
        let gc_word = unsafe { ptr::read_unaligned((raw as *const u8).sub(8).cast::<u64>()) };
        let array_word = (u64::from(capacity) << 32) | u64::from(length);
        unsafe {
            out.add(0).write(1);
            out.add(1).write(gc_word);
            out.add(2).write(array_word);
            out.add(3).write(0);
            out.add(4).write(0);
            out.add(5).write(0);
            out.add(6).write(u64::from(bound));
        }
        return Some((1, raw));
    }

    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let object = raw.cast::<ObjectHeader>();
    let Some(layout) = dense_layout_for_validated_object(object) else {
        return None;
    };
    // Stable-loop codegen already handles both inline and object-owned spill
    // element slots. `layout_length_value` below has the same split for the
    // semantic length slot, so classes with enough declared fields to spill
    // `length` (the real wolf-ecs Query/Archetype shape) are equally safe.
    if !crate::object::object_spill_enabled() {
        return None;
    }
    let Some(length) = nonnegative_u32_length(layout_length_value(object, layout)) else {
        return None;
    };
    let bound = requested_bound.unwrap_or(length);
    if bound > length || bound > layout.dense_prefix_len || length > 16_000_000 {
        return None;
    }
    if require_numeric >= 2 {
        // The direct ECS clone uses one preheader base. Reject the uncommon
        // layout whose admitted prefix straddles inline object fields and the
        // object-owned spill array; the unchanged generic clone handles it.
        let Some(end_slot) = layout.element_base.checked_add(bound) else {
            return None;
        };
        if layout.element_base < layout.live_inline_slots && end_slot > layout.live_inline_slots {
            return None;
        }
    }
    if require_numeric != 0 {
        if !unsafe { ensure_subclass_numeric_prefix(object, layout, bound, require_numeric >= 2) } {
            return None;
        }
    }
    let gc_word = unsafe { ptr::read_unaligned((raw as *const u8).sub(8).cast::<u64>()) };
    let receiver_word = unsafe { ptr::read_unaligned(raw.cast::<u64>()) };
    unsafe {
        out.add(0).write(2);
        out.add(1).write(gc_word);
        out.add(2).write(receiver_word);
        out.add(3).write(u64::from(layout.length_slot));
        out.add(4).write(u64::from(layout.element_base));
        out.add(5).write(
            (u64::from(layout.live_inline_slots) << 32) | u64::from(layout.dense_prefix_len),
        );
        out.add(6).write(u64::from(bound));
    }
    Some((2, raw))
}

#[no_mangle]
pub extern "C" fn js_packed_arraylike_loop_guard(
    receiver: f64,
    bound: f64,
    require_numeric: i32,
    out: *mut u64,
) -> i32 {
    packed_arraylike_loop_guard(receiver, bound, require_numeric, out)
        .map(|(kind, _)| kind)
        .unwrap_or(0)
}

/// #8773 capture-safe packed-loop admission. In addition to filling the seven
/// scalar descriptor words, return the live receiver user address. The caller
/// consumes it before the next safepoint and reloads/revalidates on the next
/// iteration; the returned address is never stored as a GC root.
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_packed_arraylike_loop_guard_live(
    receiver: f64,
    bound: f64,
    require_numeric: i32,
    out: *mut u64,
) -> i64 {
    packed_arraylike_loop_guard(receiver, bound, require_numeric, out)
        .map(|(_, raw)| raw as i64)
        .unwrap_or(0)
}

/// Fused admission for the call-free ECS swap clone. The source layout and
/// exact-u32 prefix are validated by the packed-loop guard, while two to four
/// erased component columns must be pairwise-distinct owning Uint32Arrays.
/// The first seven output words retain the ordinary source descriptor; words
/// 7..10 receive up to four stable component header addresses. A zero return
/// leaves the complete operation to the unchanged generic loop.
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_packed_ecs_u32_loop_guard(
    receiver: f64,
    bound: f64,
    column0: f64,
    column1: f64,
    column2: f64,
    column3: f64,
    column_count: i32,
    out: *mut u64,
) -> i64 {
    if out.is_null() || !(2..=4).contains(&column_count) {
        return 0;
    }
    let Some((_, live_raw)) = packed_arraylike_loop_guard(receiver, bound, 2, out) else {
        return 0;
    };
    let columns = [column0, column1, column2, column3];
    let mut addresses = [0usize; 4];
    let mut common_length = None;
    for index in 0..column_count as usize {
        let address = crate::typedarray::inline_u32_addr(columns[index]);
        if address == 0 || addresses[..index].contains(&address) {
            return 0;
        }
        let length = unsafe { (*(address as *const crate::typedarray::TypedArrayHeader)).length };
        if common_length.is_some_and(|common| common != length) {
            return 0;
        }
        common_length = Some(length);
        addresses[index] = address;
    }
    // The admitted bound (`out[6]`, resolved for the live-length form too) is
    // checked against the SOURCE receiver above; the fused loop also reads
    // every column up to that bound, so a column shorter than it must decline
    // admission rather than read past its payload.
    let admitted_bound = unsafe { out.add(6).read() };
    if common_length.is_some_and(|common| u64::from(common) < admitted_bound) {
        return 0;
    }
    unsafe {
        for (index, address) in addresses
            .iter()
            .copied()
            .take(column_count as usize)
            .enumerate()
        {
            out.add(7 + index).write(address as u64);
        }
    }
    live_raw as i64
}

/// Revalidate a receiver against facts published by a successful complete
/// loop admission. Unlike the admitting guard, this path never rediscovers or
/// republishes the dense layout: exact class/ShapeId and header checks make the
/// seven scalar words an O(1) semantic version token. It is used between
/// observable indexed effects in a nested fast loop.
///
/// The receiver is live and rooted at the generated call site. One retained
/// Array-growth forwarding edge is accepted, while every brand, shape,
/// prototype, descriptor, length, packedness, or numeric-proof mismatch
/// returns zero to the exact-source generic read.
#[no_mangle]
pub extern "C" fn js_packed_arraylike_loop_revalidate_live(
    receiver: f64,
    bound: f64,
    require_numeric: i32,
    facts: *const u64,
) -> i64 {
    if facts.is_null() {
        return 0;
    }
    // The public Wolf hot path is an admitted Array-subclass. Its descriptor
    // is already the authority for the exact class/ShapeId and dense layout;
    // do not repeat generic pointer classification, forwarding triage, cache
    // lookup, or bound parsing before checking those words. A genuine JS heap
    // pointer is the only value that can pass `is_pointer`, so its prepended
    // GcHeader is safe to inspect directly. Plain Arrays retain the complete
    // defensive path below because their growth forwarding stubs are valid.
    if unsafe { facts.read() } == 2 {
        return revalidate_admitted_subclass_live(receiver, bound, require_numeric, facts);
    }
    if unsafe { facts.read() } == 3 {
        return revalidate_admitted_elements_live(receiver, bound, require_numeric, facts);
    }
    let live_length_bound = bound == -1.0;
    let js = JSValue::from_bits(receiver.to_bits());
    if !js.is_pointer() {
        return 0;
    }
    let source = js.as_pointer::<u8>();
    let Some(source_header) =
        (unsafe { crate::value::addr_class::try_read_gc_header(source as usize) })
    else {
        return 0;
    };
    // Re-resolve an elements-backed receiver to its CURRENT inner array: an
    // append inside the loop body may have re-allocated it, and the meta slot
    // is the authority.
    let (source, source_header) =
        elements_loop_source(source, source_header).unwrap_or((source, source_header));
    let raw = if source_header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        if source_header.obj_type != crate::gc::GC_TYPE_ARRAY {
            return 0;
        }
        let target = unsafe { crate::gc::forwarding_address(source_header) };
        let Some(target_header) =
            (unsafe { crate::value::addr_class::try_read_gc_header(target as usize) })
        else {
            return 0;
        };
        if target_header.obj_type != crate::gc::GC_TYPE_ARRAY
            || target_header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return 0;
        }
        target
    } else {
        source
    };
    let Some(header) = (unsafe { crate::value::addr_class::try_read_gc_header(raw as usize) })
    else {
        return 0;
    };
    let (kind, receiver_word, length_slot, element_base, packed_bounds, admitted_bound) = unsafe {
        (
            facts.add(0).read(),
            facts.add(2).read(),
            facts.add(3).read() as u32,
            facts.add(4).read() as u32,
            facts.add(5).read(),
            facts.add(6).read() as u32,
        )
    };
    if admitted_bound > 16_000_000 {
        return 0;
    }
    if !live_length_bound
        && (!bound.is_finite()
            || bound < 0.0
            || bound.fract() != 0.0
            || bound != f64::from(admitted_bound))
    {
        return 0;
    }

    if kind == 1 {
        if header.obj_type != crate::gc::GC_TYPE_ARRAY
            || header._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
            || super::super::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.load(Ordering::Relaxed) != 0
        {
            return 0;
        }
        let array = raw.cast::<ArrayHeader>();
        let (length, capacity) = unsafe { ((*array).length, (*array).capacity) };
        if (live_length_bound && length != admitted_bound)
            || (!live_length_bound && length < admitted_bound)
            || length > capacity
            || capacity > 16_000_000
            || require_numeric >= 2
            || (require_numeric != 0 && header._reserved & crate::gc::GC_ARRAY_RAW_F64_LAYOUT == 0)
        {
            return 0;
        }
        return raw as i64;
    }

    if kind != 2
        || header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return 0;
    }
    let object = raw.cast::<ObjectHeader>();
    let current_receiver_word = unsafe { ptr::read_unaligned(raw.cast::<u64>()) };
    if current_receiver_word != receiver_word
        || crate::object::prototype_chain::object_has_prototype_override(raw as usize)
    {
        return 0;
    }
    let dense_prefix_len = packed_bounds as u32;
    let live_inline_slots = (packed_bounds >> 32) as u32;
    if admitted_bound > dense_prefix_len {
        return 0;
    }
    let layout = DenseSubclassLayout {
        length_slot,
        element_base,
        dense_prefix_len,
        live_inline_slots,
    };
    let Some(length) = nonnegative_u32_length(layout_length_value(object, layout)) else {
        return 0;
    };
    if (live_length_bound && length != admitted_bound)
        || (!live_length_bound && length < admitted_bound)
        || (require_numeric != 0
            && !unsafe {
                subclass_numeric_prefix_is_proven(
                    object,
                    (*object).parent_class_id,
                    admitted_bound,
                    require_numeric >= 2,
                )
            })
    {
        return 0;
    }
    raw as i64
}

/// Kind-3 revalidation: re-resolve the elements store from the receiver and
/// prove it is the SAME payload the guard admitted (same header word, same
/// length/capacity). A moving GC changes only the address, so word 3 is
/// refreshed and the loop keeps running; a re-allocating append changes the
/// capacity word and takes the side exit, exactly as a grown plain Array does.
#[inline(always)]
fn revalidate_admitted_elements_live(
    receiver: f64,
    bound: f64,
    require_numeric: i32,
    facts: *const u64,
) -> i64 {
    let js = JSValue::from_bits(receiver.to_bits());
    if !js.is_pointer() {
        return 0;
    }
    let raw = js.as_pointer::<u8>();
    let Some(header) = (unsafe { crate::value::addr_class::try_read_gc_header(raw as usize) })
    else {
        return 0;
    };
    let Some((elements, elements_header)) = elements_loop_source(raw, header) else {
        return 0;
    };
    if elements_header._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
        || super::super::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.load(Ordering::Relaxed) != 0
    {
        return 0;
    }
    let (admitted_gc_word, admitted_array_word, admitted_bound) = unsafe {
        (
            facts.add(1).read(),
            facts.add(2).read(),
            facts.add(6).read(),
        )
    };
    let array = elements.cast::<ArrayHeader>();
    let (length, capacity) = unsafe { ((*array).length, (*array).capacity) };
    let array_word = (u64::from(capacity) << 32) | u64::from(length);
    let gc_word = unsafe { ptr::read_unaligned(elements.sub(8).cast::<u64>()) };
    if array_word != admitted_array_word || gc_word != admitted_gc_word {
        return 0;
    }
    let live_length_bound = bound == -1.0;
    if !live_length_bound
        && (!bound.is_finite()
            || bound < 0.0
            || bound.fract() != 0.0
            || bound != admitted_bound as f64)
    {
        return 0;
    }
    if admitted_bound > u64::from(length) {
        return 0;
    }
    if require_numeric != 0
        && !unsafe { super::super::header::ensure_array_numeric_raw_f64(array as *mut ArrayHeader) }
    {
        return 0;
    }
    // Republish the payload address: the store itself may have been evacuated
    // even though its contents and header word are unchanged.
    unsafe {
        (facts as *mut u64).add(3).write(elements as u64);
        (facts as *mut u64)
            .add(6)
            .write(u64::from(length).min(admitted_bound));
    }
    raw as i64
}

#[inline(always)]
fn revalidate_admitted_subclass_live(
    receiver: f64,
    bound: f64,
    require_numeric: i32,
    facts: *const u64,
) -> i64 {
    let js = JSValue::from_bits(receiver.to_bits());
    if !js.is_pointer() {
        return 0;
    }
    let raw = js.as_pointer::<u8>();
    let header = unsafe {
        &*raw
            .sub(crate::gc::GC_HEADER_SIZE)
            .cast::<crate::gc::GcHeader>()
    };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return 0;
    }
    let object = raw.cast::<ObjectHeader>();
    let (receiver_word, length_slot, element_base, packed_bounds, admitted_bound) = unsafe {
        (
            facts.add(2).read(),
            facts.add(3).read() as u32,
            facts.add(4).read() as u32,
            facts.add(5).read(),
            facts.add(6).read() as u32,
        )
    };
    if admitted_bound > 16_000_000
        || unsafe { ptr::read_unaligned(raw.cast::<u64>()) } != receiver_word
    {
        return 0;
    }
    let meta = unsafe { (*object).meta };
    if !meta.is_null()
        && unsafe { (*meta).flags } & crate::object::OBJECT_META_FLAG_PROTO_OVERRIDE != 0
    {
        return 0;
    }
    let dense_prefix_len = packed_bounds as u32;
    let live_inline_slots = (packed_bounds >> 32) as u32;
    if admitted_bound > dense_prefix_len {
        return 0;
    }
    let layout = DenseSubclassLayout {
        length_slot,
        element_base,
        dense_prefix_len,
        live_inline_slots,
    };
    let Some(length) = nonnegative_u32_length(layout_length_value(object, layout)) else {
        return 0;
    };
    if (bound == -1.0 && length != admitted_bound)
        || (bound != -1.0 && length < admitted_bound)
        || (require_numeric != 0
            && !unsafe {
                subclass_numeric_prefix_is_proven(
                    object,
                    (*object).parent_class_id,
                    admitted_bound,
                    require_numeric >= 2,
                )
            })
    {
        return 0;
    }
    raw as i64
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_PACKED_ARRAYLIKE_LOOP_GUARD: extern "C" fn(f64, f64, i32, *mut u64) -> i32 =
    js_packed_arraylike_loop_guard;

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_PACKED_ARRAYLIKE_LOOP_GUARD_LIVE: extern "C" fn(f64, f64, i32, *mut u64) -> i64 =
    js_packed_arraylike_loop_guard_live;

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_PACKED_ARRAYLIKE_LOOP_REVALIDATE_LIVE: extern "C" fn(
    f64,
    f64,
    i32,
    *const u64,
) -> i64 = js_packed_arraylike_loop_revalidate_live;
