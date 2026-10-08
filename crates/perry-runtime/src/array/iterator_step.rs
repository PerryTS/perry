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
    let proto = crate::array::array_prototype_addr_if_resolved();
    if proto != 0 && crate::object::iterator_prototypes_materialized() {
        return array_record_needs_iterator_resolved(value, proto, std::ptr::null_mut());
    }
    array_record_needs_iterator_cold(value, std::ptr::null_mut())
}

/// Capture the actual source and its shape verdict together. The output is
/// published only after intrinsic materialization has finished allocating.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_enter(value: f64, out: *mut f64) -> i32 {
    let proto = crate::array::array_prototype_addr_if_resolved();
    if proto != 0 && crate::object::iterator_prototypes_materialized() {
        return array_record_needs_iterator_resolved(value, proto, out);
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
        out,
    )
}

unsafe fn array_record_needs_iterator_resolved(value: f64, proto: usize, out: *mut f64) -> i32 {
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
    // An alias can still name a growth stub. Its old flags do not describe
    // members subsequently installed on the live array.
    let array = crate::array::clean_arr_ptr(raw as *const crate::array::ArrayHeader);
    if !array.is_null() && !out.is_null() {
        // GC_STORE_AUDIT(STACK): caller owns the repaired live source root.
        *out = crate::value::js_nanbox_pointer(array as i64);
    }
    if array.is_null()
        || crate::array::array_object_flags_resolved(array) & crate::gc::GC_ARRAY_CUSTOM_PROTO != 0
    {
        return 1;
    }
    let symbol = crate::symbol::well_known_symbol("iterator");
    if crate::array::array_object_flags_resolved(array) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
    {
        let own = crate::array::array_property_bag(array);
        if !own.is_null() {
            if crate::object::shaped_symbols::position(own, symbol as usize).is_some() {
                return 1;
            }
        }
    }
    array_record_prototypes_need_iterator(proto, symbol as usize)
}

#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_literal_needs_iterator() -> i32 {
    let mut proto_addr = crate::array::array_prototype_addr_if_resolved();
    if proto_addr == 0 || !crate::object::iterator_prototypes_materialized() {
        let _ = crate::object::builtin_prototype_value("Array");
        crate::object::ensure_iterator_prototypes();
        proto_addr = crate::array::array_prototype_addr();
    }
    let symbol = crate::symbol::well_known_symbol("iterator");
    array_record_prototypes_need_iterator(proto_addr, symbol as usize)
}

unsafe fn array_record_prototypes_need_iterator(proto_addr: usize, symbol: usize) -> i32 {
    // The intrinsic root is an Array, and its actual property bag owns the
    // symbol shape. Avoid rediscovering that known receiver brand through
    // the generic descriptor router. No mutation fact is cached here.
    let proto = crate::array::array_property_bag(proto_addr as *const crate::array::ArrayHeader);
    if proto.is_null() {
        return 1;
    }
    let Some(shape) = crate::object::shapes::object_shape_record(proto) else {
        return 1;
    };
    let key_bits = crate::value::POINTER_TAG | symbol as u64;
    let keys = shape.keys() as *const crate::array::ArrayHeader;
    let slots = crate::array::array_elements_ptr(keys);
    let canonical = crate::object::array_prototype_values_thunk as *const u8;
    let member = (0..shape.logical_key_count()).find(|&i| *slots.add(i as usize) == key_bits);
    let Some(slot) = member else {
        return 1;
    };
    let Some(info) = shape.constfn_info(slot) else {
        return 1;
    };
    if (*(info as *const crate::closure::JsFunctionInfo)).code != canonical {
        return 1;
    }
    i32::from(!crate::object::array_record_next_is_builtin())
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
