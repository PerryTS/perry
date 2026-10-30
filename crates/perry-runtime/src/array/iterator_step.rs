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
    if let Some(header) = crate::value::addr_class::pointer_value_header(iter.to_bits()) {
        let raw = (iter.to_bits() & crate::value::POINTER_MASK) as usize;
        {
            if crate::gc_type_is_object!(header.obj_type) {
                let obj = raw as *mut ObjectHeader;
                // Only these layouts can use the builtin collection step. A
                // generator's branded frame goes directly to resume below;
                // do not resolve an unrelated iterator-family owner first.
                if matches!(
                    (*obj).class_id,
                    crate::buffer::BUFFER_ITERATOR_CLASS_ID
                        | crate::array::ARRAY_ITERATOR_CLASS_ID
                        | crate::collection_iter_object::MAP_ITERATOR_CLASS_ID
                        | crate::collection_iter_object::SET_ITERATOR_CLASS_ID
                        | crate::string::STRING_ITERATOR_CLASS_ID
                ) && crate::object::iterator_step_method_is_builtin(obj, next)
                {
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
    // A sync generator stepped through its captured intrinsic `next`: the
    // call is its record's resume, and the step comes back as its raw value
    // and kind, with no IteratorResult object. Only a `yield*` inner result
    // re-yielded by the body is an object, read by ordinary [[Get]].
    if let Some(step) =
        crate::object::resume_intrinsic_sync_step(iter, next, f64::from_bits(TAG_UNDEFINED))
    {
        if step.kind == crate::object::STEP_INNER_RESULT {
            return step_from_result(step.value, out);
        }
        let done = step.kind == crate::object::STEP_DONE;
        if !out.is_null() {
            // GC_STORE_AUDIT(STACK): publish to the caller's native frame slot after reentry.
            *out = if done {
                f64::from_bits(TAG_UNDEFINED)
            } else {
                step.value
            };
        }
        return i32::from(done);
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
    step_from_result(result, out)
}

/// IteratorComplete then (when not done) IteratorValue of a returned result:
/// validated as an Object, members read by ordinary [[Get]].
unsafe fn step_from_result(result: f64, out: *mut f64) -> i32 {
    let scope = crate::gc::RuntimeHandleScope::new();
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
    iterator_next_method_rooted(&iter)
}

/// Admission's captured iterator is already rooted. Read next from its real
/// family slot, including a replacement or accessor, without a second proof
/// or handle scope. Own properties and foreign edges keep ordinary [[Get]].
pub(super) unsafe fn iterator_next_method_rooted(iter: &crate::gc::RuntimeHandle<'_>) -> f64 {
    let raw = js_nanbox_get_pointer(iter.get_nanbox_f64()) as usize;
    if crate::value::addr_class::pointer_value_header(iter.get_nanbox_u64())
        .is_some_and(|h| crate::gc_type_is_object!(h.obj_type))
    {
        let obj = raw as *const ObjectHeader;
        // An actual own next slot needs only its descriptor. In particular,
        // ordinary custom iterators must not bind a method during [[Get]];
        // the following Call supplies the iterator as its receiver.
        if crate::object::shapes::object_shape_record(obj).is_some() {
            let keys = crate::object::object_keys(obj);
            let (slots, count) = keys.dense_slots();
            if let Some(slot) = (0..count).find(|&i| {
                crate::string::js_string_key_matches_bytes(
                    JSValue::from_bits((*slots.add(i)).to_bits()),
                    b"next",
                )
            }) {
                return super::iterator_method::read_method(
                    iter,
                    super::iterator_method::property_at(obj, slot as u32),
                );
            }
        }
        if let Some(owner) = crate::object::iterator_next_property_owner(obj) {
            let keys = crate::object::object_keys(owner);
            let (slots, count) = keys.dense_slots();
            if let Some(slot) = (0..count).find(|&i| {
                crate::string::js_string_key_matches_bytes(
                    JSValue::from_bits((*slots.add(i)).to_bits()),
                    b"next",
                )
            }) {
                let method = super::iterator_method::property_at(owner, slot as u32);
                return super::iterator_method::read_method(iter, method);
            }
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
    array_record_enter(
        value,
        std::ptr::null_mut(),
        super::array_iteration_facts(),
        false,
        std::ptr::null_mut(),
    )
}

/// Capture the actual source and its shape verdict together. The output is
/// published only after intrinsic materialization has finished allocating.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_enter(
    value: f64,
    out: *mut f64,
    iterator_writable: u32,
) -> i32 {
    array_record_enter(value, out, iterator_writable, false, std::ptr::null_mut())
}

/// The counted consumer's entry: the same proof (bit 0), then the packed-f64
/// loop admission of the live head the proof already resolved (bit 1). The
/// loop consumes bit 1 in place of a second receiver classification.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_enter_counted(
    value: f64,
    out: *mut f64,
    iterator_writable: u32,
) -> i32 {
    array_record_enter(value, out, iterator_writable, true, std::ptr::null_mut())
}

/// Admission and protocol acquisition share the receiver/owner facts. The
/// caller publishes all six words before entry; only a refused proof reenters.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_stack_enter(
    record: *mut super::iterator_stack_record::ArrayStackRecord,
    iterator_writable: u32,
    counted: u32,
) -> i32 {
    let value = std::ptr::read_volatile(std::ptr::addr_of!((*record).payload));
    array_record_enter(
        value,
        std::ptr::addr_of_mut!((*record).payload),
        iterator_writable,
        counted != 0,
        record,
    )
}

#[cold]
unsafe fn array_record_refused(
    value: f64,
    record: *mut super::iterator_stack_record::ArrayStackRecord,
    method: Option<super::iterator_method::IteratorMethod>,
) -> i32 {
    if record.is_null() {
        return 1;
    }
    // Transfer custody before any user call: its acquisition roots the receiver,
    // and unwinding skips Rust cleanup. No allocation precedes that transfer.
    super::iterator_stack_record::clear_record(record);
    let iter = match method {
        Some(method) => super::iterator_method::iterator_from_method(value, method),
        None => crate::symbol::js_get_iterator(value),
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let iter = scope.root_nanbox_f64(iter);
    let next = iterator_next_method_rooted(&iter);
    // GC_STORE_AUDIT(STACK): publish the sole captured IteratorRecord after reentry.
    std::ptr::write_volatile(
        std::ptr::addr_of_mut!((*record).payload),
        iter.get_nanbox_f64(),
    );
    // GC_STORE_AUDIT(STACK): publish captured next into the caller's root range.
    std::ptr::write_volatile(std::ptr::addr_of_mut!((*record).next), next);
    1
}

#[inline(always)]
unsafe fn array_record_enter(
    value: f64,
    out: *mut f64,
    iterator_writable: u32,
    counted: bool,
    record: *mut super::iterator_stack_record::ArrayStackRecord,
) -> i32 {
    if !out.is_null() {
        *out = value;
    }
    if !JSValue::from_bits(value.to_bits()).is_pointer() {
        return array_record_refused(value, record, None);
    }
    let raw = js_nanbox_get_pointer(value) as usize;
    let Some(header) = crate::value::addr_class::try_read_gc_header(raw) else {
        return array_record_refused(value, record, None);
    };
    if header.obj_type != crate::gc::GC_TYPE_ARRAY {
        return array_record_refused(value, record, None);
    }
    let array = crate::array::clean_arr_ptr(raw as *const crate::array::ArrayHeader);
    let value = crate::value::js_nanbox_pointer(array as i64);
    if !out.is_null() {
        *out = value;
    }
    if iterator_writable & super::ITERATION_MEMBER_WRITABLE != 0 {
        let method = array_iterator_property(array);
        if !matches!(method, super::iterator_method::IteratorMethod::Intrinsic) {
            return array_record_refused(value, record, Some(method));
        }
    }
    if iterator_writable & super::ITERATION_NEXT_REACHABLE != 0
        && !crate::object::iterator_prototypes::array_iterator_next_is_canonical()
    {
        return array_record_refused(
            value,
            record,
            Some(super::iterator_method::IteratorMethod::Intrinsic),
        );
    }
    let flags = crate::array::array_object_flags_resolved(array);
    if counted && crate::typed_feedback::packed_f64_loop_admits_live_array(array, flags) {
        return 2;
    }
    0
}

/// The iterator property is the sole semantic dispatch for array iteration.
/// Standalone runtime callers and programs with reachable writes read it;
/// immutable programs select the intrinsic arm at compile time.
pub(crate) unsafe fn array_iterator_property(
    array: *const crate::array::ArrayHeader,
) -> super::iterator_method::IteratorMethod {
    super::iterator_method::array_iterator_method(
        array,
        crate::array::array_prototype_addr_if_resolved(),
    )
}

/// Is iterating `array` the builtin element walk under the program's
/// iteration `facts` (`indexing_support::ARRAY_ITERATION_FACTS`)? With none
/// it is, unchecked; otherwise the array's iteration member (one fixed slot
/// of `%Array.prototype%`'s layout, or the array's own) must be the
/// intrinsic, and `%ArrayIteratorPrototype%.next` the builtin.
pub(crate) unsafe fn array_iteration_is_intrinsic_under(
    array: *const crate::array::ArrayHeader,
    facts: u32,
) -> bool {
    (facts & super::ITERATION_MEMBER_WRITABLE == 0
        || matches!(
            array_iterator_property(array),
            super::iterator_method::IteratorMethod::Intrinsic
        ))
        && (facts & super::ITERATION_NEXT_REACHABLE == 0
            || crate::object::iterator_prototypes::array_iterator_next_is_canonical())
}

/// [`array_iteration_is_intrinsic_under`] the program's own facts.
pub(crate) unsafe fn array_iteration_is_intrinsic(array: *const crate::array::ArrayHeader) -> bool {
    array_iteration_is_intrinsic_under(array, super::array_iteration_facts())
}

#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_literal_needs_iterator(facts: u32) -> i32 {
    if facts == 0 {
        return 0;
    }
    let proto = crate::object::builtin_prototype_value("Array");
    let array = js_nanbox_get_pointer(proto) as *const crate::array::ArrayHeader;
    i32::from(!array_iteration_is_intrinsic_under(array, facts))
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
    array_record_close(
        value,
        index,
        crate::value::js_is_truthy(done) != 0,
        error,
        crate::value::js_is_truthy(throwing) != 0,
    )
}

#[inline(never)]
pub(super) unsafe fn array_record_close(
    value: f64,
    index: f64,
    done: bool,
    error: f64,
    throwing: bool,
) -> f64 {
    if done
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
    if done || crate::object::array_record_close_is_absent() {
        return if throwing {
            error.get_nanbox_f64()
        } else {
            f64::from_bits(TAG_UNDEFINED)
        };
    }
    let iter = js_array_record_iterator_at(value.get_nanbox_f64(), index);
    if throwing {
        crate::array::js_iterator_close_on_throw(
            iter,
            f64::from_bits(crate::value::TAG_FALSE),
            error.get_nanbox_f64(),
        )
    } else {
        crate::array::js_iterator_close_if_not_done(iter, f64::from_bits(crate::value::TAG_FALSE))
    }
}
