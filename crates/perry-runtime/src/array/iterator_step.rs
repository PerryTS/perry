//! Native IteratorStepValue for compiler-owned consumers.
use crate::iter_result::IteratorStep;
use crate::object::ObjectHeader;
use crate::value::{js_nanbox_get_pointer, JSValue, TAG_UNDEFINED};

/// The out slot belongs to the generated frame. Write it only after all
/// allocating/reentrant work is finished, so no unrooted value crosses a GC.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_iterator_step(iter: f64, next: f64, out: *mut f64) -> i32 {
    let mut step = IteratorStep {
        value: f64::from_bits(TAG_UNDEFINED),
        done: true,
    };
    if JSValue::from_bits(iter.to_bits()).is_pointer() {
        let raw = js_nanbox_get_pointer(iter) as usize;
        if let Some(header) = crate::value::addr_class::try_read_gc_header(raw) {
            if header.obj_type == crate::gc::GC_TYPE_OBJECT {
                let obj = raw as *mut ObjectHeader;
                if crate::object::iterator_step_method_is_builtin(obj, next) {
                    match (*obj).class_id {
                        crate::buffer::BUFFER_ITERATOR_CLASS_ID => {
                            crate::buffer::dispatch_buffer_iterator_step(obj, &mut step)
                        }
                        crate::array::ARRAY_ITERATOR_CLASS_ID => {
                            crate::array::dispatch_array_iterator_step(obj, &mut step)
                        }
                        crate::collection_iter_object::MAP_ITERATOR_CLASS_ID
                        | crate::collection_iter_object::SET_ITERATOR_CLASS_ID => {
                            crate::collection_iter_object::dispatch_collection_iterator_step(
                                obj, &mut step,
                            )
                        }
                        crate::string::STRING_ITERATOR_CLASS_ID => {
                            crate::string::dispatch_string_iterator_step(obj, &mut step)
                        }
                        _ => unreachable!(),
                    }
                    if !out.is_null() {
                        // GC_STORE_AUDIT(STACK): publish to the caller's native frame slot after reentry.
                        *out = step.value;
                    }
                    return i32::from(step.done);
                }
            }
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let iter = scope.root_nanbox_f64(iter);
    let next = scope.root_nanbox_f64(next);
    if !crate::proxy::is_callable_function(next.get_nanbox_f64()) {
        crate::closure::throw_not_callable();
    }
    let result = crate::closure::native_call_value_this(
        next.get_nanbox_f64(),
        crate::closure::JsThis::from_f64(iter.get_nanbox_f64()),
        std::ptr::null(),
        0,
    );
    let result = scope.root_nanbox_f64(crate::symbol::js_iterator_result_validate(result));
    let field = |name: &[u8]| {
        let key = crate::string::intern_ascii_literal(name);
        crate::object::js_object_get_field_by_name_f64(
            js_nanbox_get_pointer(result.get_nanbox_f64()) as *const ObjectHeader,
            key,
        )
    };
    let done = crate::value::js_is_truthy(field(b"done")) != 0;
    let value = if done || out.is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        field(b"value")
    };
    if !out.is_null() {
        // GC_STORE_AUDIT(STACK): publish to the caller's native frame slot after reentry.
        *out = value;
    }
    i32::from(done)
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_ITERATOR_STEP: unsafe extern "C-unwind" fn(f64, f64, *mut f64) -> i32 =
    js_iterator_step;

/// Get the iterator record's next method once, before any step. A pristine
/// shape already proves an ordinary data read, so no binding object is needed.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_iterator_next_method(iter: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let iter = scope.root_nanbox_f64(iter);
    let raw = js_nanbox_get_pointer(iter.get_nanbox_f64()) as usize;
    if crate::value::addr_class::try_read_gc_header(raw)
        .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_OBJECT)
    {
        let obj = raw as *const ObjectHeader;
        if crate::object::iterator_step_is_builtin(obj) {
            let proto = js_nanbox_get_pointer(f64::from_bits(
                crate::object::shapes::object_prototype_word(obj),
            )) as *const ObjectHeader;
            return f64::from_bits(crate::object::js_object_get_field(proto, 0).bits());
        }
    }
    let key = crate::string::intern_ascii_literal(b"next");
    crate::object::js_object_get_field_by_name_f64(
        js_nanbox_get_pointer(iter.get_nanbox_f64()) as *const ObjectHeader,
        key,
    )
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_ITERATOR_NEXT_METHOD: unsafe extern "C-unwind" fn(f64) -> f64 = js_iterator_next_method;

/// Destructuring rest consumes the same iterator record and step routine.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_iterator_step_rest_to_array(
    iter: f64,
    next: f64,
    done: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let iter = scope.root_nanbox_f64(iter);
    let next = scope.root_nanbox_f64(next);
    let arr = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
    if crate::value::js_is_truthy(done) == 0 {
        loop {
            let mut value = f64::from_bits(TAG_UNDEFINED);
            if js_iterator_step(iter.get_nanbox_f64(), next.get_nanbox_f64(), &mut value) != 0 {
                break;
            }
            let item_scope = crate::gc::RuntimeHandleScope::new();
            let value = item_scope.root_nanbox_f64(value);
            arr.with_mut_ptr(|a| crate::array::js_array_push_f64(a, value.get_nanbox_f64()));
        }
    }
    arr.with_mut_ptr::<crate::array::ArrayHeader, _>(|a| crate::value::js_nanbox_pointer(a as i64))
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_ITERATOR_STEP_REST: unsafe extern "C-unwind" fn(f64, f64, f64) -> f64 =
    js_iterator_step_rest_to_array;

/// Materialize already evaluated dense literal values only when the protocol
/// or an observable close needs their receiver. Root the entire pack before
/// the first allocation; no borrowed caller word crosses a collection.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_literal(values: *const f64, count: u32) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let values: Vec<_> = (0..count as usize)
        .map(|i| scope.root_nanbox_f64(*values.add(i)))
        .collect();
    let array = scope.root_raw_mut_ptr(crate::array::js_array_alloc(count));
    for value in values {
        let next =
            array.with_mut_ptr(|a| crate::array::js_array_push_f64(a, value.get_nanbox_f64()));
        array.set_raw_mut_ptr(next);
    }
    array.with_const_ptr::<crate::array::ArrayHeader, _>(|a| {
        crate::value::js_nanbox_pointer(a as i64)
    })
}

/// Entry proof for the array representation of an IteratorRecord. Own array
/// keys/reparenting are already described by the array's header shape word.
/// The prototype shape describes the iteration member and its ConstFn body.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_needs_iterator(value: f64) -> i32 {
    array_record_enter(value, std::ptr::null_mut(), std::ptr::null(), false)
}

/// One entry site's memo of the intrinsic owners' shapes its last full proof
/// validated, plus its verdict: the Array prototype bag's ShapeId in the low half and
/// %ArrayIteratorPrototype%'s in the high half. Its only authority is the
/// compare against both owners' current ShapeIds at every entry; ShapeIds are
/// minted from one process-wide sequence and never reused, so a word written
/// by another realm or agent can only match an owner whose shape carries the
/// same validated facts. Zero never matches (it names no shape).
pub type ArrayRecordSite = std::sync::atomic::AtomicU64;

// ShapeIds occupy [0x8000_0000, 0xc000_0000), so bit 30 is
// always zero. Bit 62 of their pair records refusal without losing either
// identity. Dictionary ids are never memoized: their keys can change in place.
pub(crate) const ARRAY_RECORD_SITE_REFUSED: u64 = 1 << 62;
const _: () = assert!(crate::object::shapes::SHAPE_ID_END <= 0xc000_0000);

/// Capture the actual source and its shape verdict together. The output is
/// published only after intrinsic materialization has finished allocating.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_enter(
    value: f64,
    out: *mut f64,
    site: *const ArrayRecordSite,
) -> i32 {
    array_record_enter(value, out, site, false)
}

/// The counted consumer's entry: the same proof (bit 0), then the packed-f64
/// loop admission of the live head the proof already resolved (bit 1). The
/// loop consumes bit 1 in place of a second receiver classification.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_enter_counted(
    value: f64,
    out: *mut f64,
    site: *const ArrayRecordSite,
) -> i32 {
    array_record_enter(value, out, site, true)
}

#[inline(always)]
unsafe fn array_record_enter(
    value: f64,
    out: *mut f64,
    site: *const ArrayRecordSite,
    counted: bool,
) -> i32 {
    let proto = crate::array::array_prototype_addr_if_resolved();
    // Published only once the whole iterator-prototype tower is built.
    let next_owner = crate::object::array_iterator_prototype_addr();
    if proto != 0 && next_owner != 0 {
        return array_record_needs_iterator_resolved(value, proto, next_owner, out, site, counted);
    }
    array_record_needs_iterator_cold(value, out)
}

#[cold]
#[inline(never)]
unsafe fn array_record_needs_iterator_cold(value: f64, out: *mut f64) -> i32 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let _ = crate::object::builtin_prototype_value("Array");
    crate::object::ensure_iterator_prototypes();
    array_record_needs_iterator_resolved(
        value.get_nanbox_f64(),
        crate::array::array_prototype_addr(),
        crate::object::array_iterator_prototype_addr(),
        out,
        std::ptr::null(),
        false,
    )
}

#[inline(always)]
unsafe fn array_record_needs_iterator_resolved(
    value: f64,
    proto: usize,
    next_owner: usize,
    out: *mut f64,
    site: *const ArrayRecordSite,
    counted: bool,
) -> i32 {
    if !out.is_null() {
        // GC_STORE_AUDIT(STACK): publish after the cold entry has completed reentry.
        *out = value;
    }
    if !JSValue::from_bits(value.to_bits()).is_pointer() {
        return 1;
    }
    let raw = js_nanbox_get_pointer(value) as usize;
    let Some(header) = crate::value::addr_class::try_read_gc_header(raw) else {
        return 1;
    };
    if header.obj_type != crate::gc::GC_TYPE_ARRAY {
        return 1;
    }
    let mut array = raw as *const crate::array::ArrayHeader;
    let mut flags = header._reserved;
    if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        // An alias can still name a growth stub. Its old flags do not
        // describe members subsequently installed on the live array.
        array = crate::array::clean_arr_ptr(array);
        if array.is_null() {
            return 1;
        }
        flags = crate::array::array_object_flags_resolved(array);
        if !out.is_null() {
            // GC_STORE_AUDIT(STACK): caller owns the repaired live source root.
            *out = crate::value::js_nanbox_pointer(array as i64);
        }
    }
    if flags & crate::gc::GC_ARRAY_CUSTOM_PROTO != 0 {
        return 1;
    }
    if flags & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
        let own = crate::array::array_property_bag(array);
        let symbol = crate::symbol::well_known_symbol("iterator") as usize;
        if !own.is_null() && crate::object::shaped_symbols::position(own, symbol).is_some() {
            return 1;
        }
    }
    if !array_record_prototypes_proven_at(site, proto, next_owner) {
        return 1;
    }
    if counted && crate::typed_feedback::packed_f64_loop_admits_live_array(array, flags) {
        return 2;
    }
    0
}

#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_literal_needs_iterator(
    site: *const ArrayRecordSite,
) -> i32 {
    let mut proto_addr = crate::array::array_prototype_addr_if_resolved();
    let mut next_owner = crate::object::array_iterator_prototype_addr();
    if proto_addr == 0 || next_owner == 0 {
        let _ = crate::object::builtin_prototype_value("Array");
        crate::object::ensure_iterator_prototypes();
        proto_addr = crate::array::array_prototype_addr();
        next_owner = crate::object::array_iterator_prototype_addr();
    }
    i32::from(!array_record_prototypes_proven_at(
        site, proto_addr, next_owner,
    ))
}

/// The prototype facts at one entry site: a hit is the two owners' current
/// ShapeIds equal to the site's validated pair; anything else is the full
/// shape proof, which publishes the pair with either verdict. Every edit that can
/// change a checked fact (a store, define or delete on either member, a
/// dictionary conversion) moves its owner to another ShapeId.
#[inline(always)]
unsafe fn array_record_prototypes_proven_at(
    site: *const ArrayRecordSite,
    proto_addr: usize,
    next_owner: usize,
) -> bool {
    let bag = crate::array::array_property_bag(proto_addr as *const crate::array::ArrayHeader);
    if bag.is_null() {
        return false;
    }
    let array_shape = crate::object::shapes::object_shape_stamp(bag);
    let next_shape = crate::object::shapes::object_shape_stamp(next_owner as *const ObjectHeader);
    let pair = u64::from(array_shape) | u64::from(next_shape) << 32;
    let complete = array_shape != 0 && next_shape != 0;
    if !site.is_null() && complete {
        let memo = (*site).load(std::sync::atomic::Ordering::Relaxed);
        if memo == pair {
            return true;
        }
    }
    array_record_prototypes_miss(site, proto_addr, next_owner, pair)
}

// Test instrumentation only: a warm refusal must bypass the full body proof.
#[cfg(test)]
thread_local! {
    static ARRAY_RECORD_FULL_PROOFS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
pub(crate) fn array_record_full_proof_calls() -> usize {
    ARRAY_RECORD_FULL_PROOFS.with(std::cell::Cell::get)
}

// Key scans and ConstFn body checks live in one cold callee; a hit only
// reads the two owners' ShapeIds and the site's last verdict.
#[cold]
#[inline(never)]
unsafe fn array_record_prototypes_miss(
    site: *const ArrayRecordSite,
    proto_addr: usize,
    next_owner: usize,
    pair: u64,
) -> bool {
    let complete = pair as u32 != 0 && (pair >> 32) as u32 != 0;
    if !site.is_null()
        && complete
        && (*site).load(std::sync::atomic::Ordering::Relaxed) == pair | ARRAY_RECORD_SITE_REFUSED
    {
        return false;
    }
    let symbol = crate::symbol::well_known_symbol("iterator") as usize;
    #[cfg(test)]
    ARRAY_RECORD_FULL_PROOFS.with(|calls| calls.set(calls.get() + 1));
    let proven = array_record_prototypes_need_iterator(proto_addr, next_owner, symbol) == 0;
    if !site.is_null()
        && complete
        && crate::object::shapes::is_site_matchable_shape_id(pair as u32)
        && crate::object::shapes::is_site_matchable_shape_id((pair >> 32) as u32)
    {
        let verdict = if proven {
            pair
        } else {
            pair | ARRAY_RECORD_SITE_REFUSED
        };
        (*site).store(verdict, std::sync::atomic::Ordering::Relaxed);
    }
    proven
}

/// Both intrinsic members, read from their owners' current shapes: the
/// Array prototype's property bag names `@@iterator` at a ConstFn lane whose
/// body is `values`, and %ArrayIteratorPrototype% names `next` at a ConstFn
/// lane whose body is the intrinsic step. No mutation fact is cached here.
#[inline(always)]
unsafe fn array_record_prototypes_need_iterator(
    proto_addr: usize,
    next_owner: usize,
    symbol: usize,
) -> i32 {
    if !array_prototype_iterates_intrinsically(proto_addr, symbol) {
        return 1;
    }
    i32::from(!crate::object::array_iterator_next_is_intrinsic(
        next_owner as *const ObjectHeader,
    ))
}

/// The Array prototype bag's shape names `@@iterator` at a ConstFn lane whose
/// body is the intrinsic `values`.
#[inline(always)]
unsafe fn array_prototype_iterates_intrinsically(proto_addr: usize, symbol: usize) -> bool {
    let bag = crate::array::array_property_bag(proto_addr as *const crate::array::ArrayHeader);
    if bag.is_null() {
        return false;
    }
    let Some(shape) = crate::object::shapes::object_shape_record(bag) else {
        return false;
    };
    let key_bits = crate::value::POINTER_TAG | symbol as u64;
    let values = crate::object::array_prototype_values_thunk as *const u8;
    crate::object::shape_member_body_is(shape, |bits| bits == key_bits, values)
}

/// GetIterator(array) when the shapes prove its iteration member is the
/// intrinsic `values`: no own `@@iterator`, the ordinary Array prototype, and
/// that prototype's ConstFn lane. Calling that member with the array as
/// receiver is exactly `array_values_iter`; anything else answers `None` and
/// the caller performs the ordinary GetMethod and Call.
pub(crate) unsafe fn array_intrinsic_values_iterator(value: f64) -> Option<f64> {
    let proto = crate::array::array_prototype_addr_if_resolved();
    if proto == 0 || !JSValue::from_bits(value.to_bits()).is_pointer() {
        return None;
    }
    let raw = js_nanbox_get_pointer(value) as usize;
    let header = crate::value::addr_class::try_read_gc_header(raw)?;
    if header.obj_type != crate::gc::GC_TYPE_ARRAY {
        return None;
    }
    let mut array = raw as *const crate::array::ArrayHeader;
    let mut flags = header._reserved;
    if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        array = crate::array::clean_arr_ptr(array);
        if array.is_null() {
            return None;
        }
        flags = crate::array::array_object_flags_resolved(array);
    }
    if flags & crate::gc::GC_ARRAY_CUSTOM_PROTO != 0 {
        return None;
    }
    let symbol = crate::symbol::well_known_symbol("iterator") as usize;
    if flags & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
        let own = crate::array::array_property_bag(array);
        if !own.is_null() && crate::object::shaped_symbols::position(own, symbol).is_some() {
            return None;
        }
    }
    if !array_prototype_iterates_intrinsically(proto, symbol) {
        return None;
    }
    Some(crate::array::array_values_iter(value))
}

#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_close_absent() -> i32 {
    i32::from(crate::object::array_record_close_is_absent())
}

/// Materialize the record only when IteratorClose needs an observable receiver.
/// It retains the original values algorithm and cursor, even after next changes.
pub(crate) unsafe fn js_array_record_iterator_at(value: f64, index: f64) -> f64 {
    let iter = crate::array::array_values_iter(value);
    let obj = js_nanbox_get_pointer(iter) as *mut ObjectHeader;
    crate::object::js_object_set_field(obj, 1, JSValue::number(index));
    iter
}

/// The omitted array record uses exactly the shared IteratorClose algorithm.
/// Its absent-return shape proof avoids materializing an unobservable receiver.
#[inline(never)]
#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_close(
    value: f64,
    index: f64,
    done: f64,
    error: f64,
    throwing: f64,
) -> f64 {
    let throwing = crate::value::js_is_truthy(throwing) != 0;
    if crate::value::js_is_truthy(done) != 0
        || (crate::object::iterator_prototypes_materialized()
            && crate::array::object_prototype_addr_if_resolved() != 0
            && crate::object::array_record_close_is_absent())
    {
        return if throwing {
            error
        } else {
            f64::from_bits(TAG_UNDEFINED)
        };
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let error = scope.root_nanbox_f64(error);
    if crate::value::js_is_truthy(done) != 0 || crate::object::array_record_close_is_absent() {
        return if throwing {
            error.get_nanbox_f64()
        } else {
            f64::from_bits(TAG_UNDEFINED)
        };
    }
    let iter = js_array_record_iterator_at(value.get_nanbox_f64(), index);
    if throwing {
        crate::array::js_iterator_close_on_throw(iter, done, error.get_nanbox_f64())
    } else {
        crate::array::js_iterator_close_if_not_done(iter, done)
    }
}
