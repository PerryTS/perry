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
