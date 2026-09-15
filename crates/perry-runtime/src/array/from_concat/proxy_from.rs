//! Proxy-only Array.from: one GetMethod, with roots across user callbacks.
use super::*;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};

fn get(value: &RuntimeHandle<'_>, name: &[u8]) -> f64 {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let raw = crate::value::js_nanbox_get_pointer(value.get_nanbox_f64());
    crate::object::js_object_get_field_by_name_f64(raw as *const crate::ObjectHeader, key)
}

fn invoke(method: &RuntimeHandle<'_>, receiver: &RuntimeHandle<'_>) -> Result<f64, f64> {
    let scope = RuntimeHandleScope::new();
    let rebound = crate::closure::clone_closure_rebind_this(
        method.get_nanbox_f64().to_bits(),
        receiver.get_nanbox_f64(),
    );
    let rebound = scope.root_nanbox_f64(f64::from_bits(rebound));
    crate::collection_iter::call_with_this_capturing_throw(
        rebound.get_nanbox_f64(),
        receiver.get_nanbox_f64(),
        &[],
    )
}

fn define(result: &RuntimeHandle<'_>, index: usize, value: f64, fresh_array: bool) {
    if fresh_array {
        let raw = crate::value::js_nanbox_get_pointer(result.get_nanbox_f64());
        let grown = js_array_set_f64_extend(raw as *mut ArrayHeader, index as u32, value);
        result.set_nanbox_f64(crate::value::js_nanbox_pointer(grown as i64));
        return;
    }
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let desc = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 4));
    for name in [
        b"value".as_slice(),
        b"writable",
        b"enumerable",
        b"configurable",
    ] {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let field = if name == b"value" {
            value.get_nanbox_f64()
        } else {
            f64::from_bits(TAG_TRUE)
        };
        desc.with_mut_ptr(|ptr| crate::object::js_object_set_field_by_name(ptr, key, field));
    }
    let name = index.to_string();
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let key = scope.root_nanbox_f64(crate::value::js_nanbox_string(key as i64));
    let descriptor = desc.with_const_ptr::<crate::ObjectHeader, _>(|ptr| {
        crate::value::js_nanbox_pointer(ptr as i64)
    });
    if crate::proxy::js_reflect_define_property(
        result.get_nanbox_f64(),
        key.get_nanbox_f64(),
        descriptor,
    )
    .to_bits()
        != TAG_TRUE
    {
        throw_cannot_define_property(index);
    }
}

fn finish(result: &RuntimeHandle<'_>, len: usize, fresh_array: bool) -> f64 {
    if fresh_array {
        let raw = crate::value::js_nanbox_get_pointer(result.get_nanbox_f64());
        crate::array::js_array_set_length(raw as *mut ArrayHeader, len as f64);
    } else {
        let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
        let key = crate::value::js_nanbox_string(key as i64);
        let current = result.get_nanbox_f64();
        crate::proxy::js_put_value_set(current, key, len as f64, current, 1);
    }
    result.get_nanbox_f64()
}

pub(super) fn array_from_proxy(
    c: f64,
    items: f64,
    mapfn: f64,
    this_arg: f64,
    mapping: bool,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(c);
    let items = scope.root_nanbox_f64(items);
    let mapfn = scope.root_nanbox_f64(mapfn);
    let this_arg = scope.root_nanbox_f64(this_arg);
    let symbol = crate::symbol::well_known_symbol("iterator");
    let method = unsafe {
        crate::symbol::js_object_get_symbol_property(
            items.get_nanbox_f64(),
            crate::value::js_nanbox_pointer(symbol as i64),
        )
    };
    let method = scope.root_nanbox_f64(method);
    let iterable = !matches!(method.get_nanbox_f64().to_bits(), TAG_UNDEFINED | TAG_NULL);
    if iterable {
        resolve_callable(method.get_nanbox_f64());
    }
    let fresh_array = !is_constructor_value(constructor.get_nanbox_f64());
    let len = if iterable {
        0
    } else {
        array_like_length(items.get_nanbox_f64())
    };
    let result = if fresh_array {
        crate::value::js_nanbox_pointer(js_array_alloc(0) as i64)
    } else {
        let args = [len as f64];
        unsafe {
            crate::object::js_new_function_construct(
                constructor.get_nanbox_f64(),
                args.as_ptr(),
                if iterable { 0 } else { 1 },
            )
        }
    };
    let result = scope.root_nanbox_f64(result);
    if !iterable {
        for index in 0..len {
            let value = crate::object::js_object_get_index_polymorphic(
                items.get_nanbox_f64().to_bits() as i64,
                index as f64,
            );
            let value = if mapping {
                call_map_fn(
                    mapfn.get_nanbox_f64(),
                    this_arg.get_nanbox_f64(),
                    value,
                    index,
                )
            } else {
                value
            };
            define(&result, index, value, fresh_array);
        }
        return finish(&result, len, fresh_array);
    }
    // GetIteratorFromMethod uses the saved method even if construction mutates
    // @@iterator. The iterator record likewise reads its next method only once.
    let iter = invoke(&method, &items).unwrap_or_else(|error| crate::exception::js_throw(error));
    crate::symbol::js_iterator_result_validate(iter);
    let iter = scope.root_nanbox_f64(iter);
    let next = scope.root_nanbox_f64(get(&iter, b"next"));
    let mut index = 0;
    loop {
        let step_scope = RuntimeHandleScope::new();
        let step = invoke(&next, &iter).unwrap_or_else(|error| crate::exception::js_throw(error));
        crate::symbol::js_iterator_result_validate(step);
        let step = step_scope.root_nanbox_f64(step);
        if crate::value::js_is_truthy(get(&step, b"done")) != 0 {
            break;
        }
        let value = step_scope.root_nanbox_f64(get(&step, b"value"));
        let completion = crate::collection_iter::call_capturing_throw(|| {
            let mapped = if mapping {
                call_map_fn(
                    mapfn.get_nanbox_f64(),
                    this_arg.get_nanbox_f64(),
                    value.get_nanbox_f64(),
                    index,
                )
            } else {
                value.get_nanbox_f64()
            };
            define(&result, index, mapped, fresh_array);
            f64::from_bits(TAG_UNDEFINED)
        });
        if let Err(error) = completion {
            let error = step_scope.root_nanbox_f64(error);
            crate::collection_iter::iterator_close(iter.get_nanbox_f64());
            crate::exception::js_throw(error.get_nanbox_f64());
        }
        index += 1;
    }
    finish(&result, index, fresh_array)
}
