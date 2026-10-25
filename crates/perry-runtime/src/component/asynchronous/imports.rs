//! Pending import Promises and borrowed-resource lifetimes are owned GC edges.
use super::*;
use crate::object::owned_state_bag::{self, COMPONENT_ASYNC_IMPORTS};
use crate::promise::{Promise, PromiseState};
fn list(owner: f64) -> f64 {
    unsafe {
        owned_state_bag::load(
            object_pointer(owner).unwrap() as usize,
            COMPONENT_ASYNC_IMPORTS,
        )
    }
}
fn expire(record: f64) {
    let lifetime = field(record, b"lifetime");
    if let Some(array) = array_pointer(lifetime) {
        crate::array::js_array_set_f64(array, 0, 0.0);
    }
}
// Drop native scratch normally before propagating a managed allocation error.
fn encoded_type(ty: &Value) -> f64 {
    let bytes = serde_json::to_vec(ty).expect("WIT result type");
    let result = crate::exception::catch_js_throw(|| {
        crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
            bytes.as_ptr(),
            bytes.len() as u32,
        ) as i64)
    });
    drop(bytes);
    finish(result)
}
pub(crate) fn register(owner: f64, request: &str, value: f64, lifetime: f64, ty: &Value) {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let value = scope.root_nanbox_f64(value);
    let lifetime = scope.root_nanbox_f64(lifetime);
    let result_type = scope.root_nanbox_f64(encoded_type(ty));
    let promise =
        scope.root_raw_mut_ptr(crate::promise::js_promise_resolved(value.get_nanbox_f64()));
    crate::promise::js_promise_mark_internally_handled(promise.get_raw_mut_ptr());
    let record = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    unsafe {
        owned_state_bag::store(
            object_pointer(record.get_nanbox_f64()).unwrap() as usize,
            owned_state_bag::COMPONENT_ASYNC_RESULT_TYPE,
            result_type.get_nanbox_f64(),
        );
    }
    let token = scope.root_string_ptr(crate::string::js_string_from_bytes(
        request.as_ptr(),
        request.len() as u32,
    ));
    set_field(
        record.get_nanbox_f64(),
        b"request",
        crate::value::js_nanbox_string(
            token.get_raw_const_ptr::<crate::string::StringHeader>() as i64
        ),
    );
    set_field(
        record.get_nanbox_f64(),
        b"promise",
        crate::value::js_nanbox_pointer(promise.get_raw_mut_ptr::<Promise>() as i64),
    );
    set_field(
        record.get_nanbox_f64(),
        b"lifetime",
        lifetime.get_nanbox_f64(),
    );
    let entries = scope.root_nanbox_f64(list(owner.get_nanbox_f64()));
    if array_pointer(entries.get_nanbox_f64()).is_none() {
        entries.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc(0) as i64,
        ));
        unsafe {
            owned_state_bag::store(
                object_pointer(owner.get_nanbox_f64()).unwrap() as usize,
                COMPONENT_ASYNC_IMPORTS,
                entries.get_nanbox_f64(),
            );
        }
    }
    let array = array_pointer(entries.get_nanbox_f64()).unwrap();
    let count = unsafe { (*array).length };
    for index in 0..count {
        if object_pointer(crate::array::js_array_get_f64(array, index)).is_none() {
            crate::array::js_array_set_f64(array, index, record.get_nanbox_f64());
            return;
        }
    }
    crate::array::js_array_set_f64_extend(array, count, record.get_nanbox_f64());
}

/// Resolve ready native request channels without entering Wasm. Rejection
/// identity stays in the operation's traced state while native errors unwind.
pub(super) fn drain(operation: f64, component: f64) {
    let scope = RuntimeHandleScope::new();
    let operation = scope.root_nanbox_f64(operation);
    let component = scope.root_nanbox_f64(component);
    let entries = scope.root_nanbox_f64(list(component.get_nanbox_f64()));
    let Some(array) = array_pointer(entries.get_nanbox_f64()) else {
        return;
    };
    let count = unsafe { (*array).length };
    for index in 0..count {
        let record = scope.root_nanbox_f64(crate::array::js_array_get_f64(
            array_pointer(entries.get_nanbox_f64()).unwrap(),
            index,
        ));
        if object_pointer(record.get_nanbox_f64()).is_none() {
            continue;
        }
        let promise = scope.root_nanbox_f64(field(record.get_nanbox_f64(), b"promise"));
        let pointer =
            JSValue::from_bits(promise.get_nanbox_f64().to_bits()).as_pointer::<Promise>();
        let state = unsafe { (*pointer).state };
        if state == PromiseState::Pending {
            continue;
        }
        let value = scope.root_nanbox_f64(unsafe {
            if state == PromiseState::Fulfilled {
                (*pointer).value
            } else {
                (*pointer).reason
            }
        });
        if state == PromiseState::Rejected {
            remember(operation.get_nanbox_f64(), value.get_nanbox_f64());
        }
        let mut encoded = Vec::new();
        let mut output = Vec::new();
        let mut ty = Value::Null;
        let result = crate::exception::catch_js_throw(|| {
            let id = string(field(record.get_nanbox_f64(), b"request"))
                .parse::<u64>()
                .unwrap_or_else(|_| fail("Invalid async component request identity"));
            encoded = if state == PromiseState::Fulfilled {
                let schema = unsafe {
                    owned_state_bag::load(
                        object_pointer(record.get_nanbox_f64()).unwrap() as usize,
                        owned_state_bag::COMPONENT_ASYNC_RESULT_TYPE,
                    )
                };
                ty = serde_json::from_str(&string(schema))
                    .unwrap_or_else(|_| fail("Invalid saved WIT result type"));
                let ctx = context::get(component.get_nanbox_f64());
                serde_json::to_vec(&value::typed::result(
                    value.get_nanbox_f64(),
                    &ty,
                    unsafe { &(*ctx).world },
                    component.get_nanbox_f64(),
                    payload::state_get(operation.get_nanbox_f64(), &OPERATION, b"arguments"),
                ))
                .unwrap()
            } else {
                b"async import rejected".to_vec()
            };
            let ctx = context::get(component.get_nanbox_f64());
            let mut buffer = Buffer::default();
            let status = unsafe {
                (host::ops().perry_component_host_async_import_resolve)(
                    (*ctx).host,
                    id,
                    state == PromiseState::Fulfilled,
                    encoded.as_ptr(),
                    encoded.len(),
                    &mut buffer,
                )
            };
            output = unsafe { take(buffer) };
            if status != OK {
                fail(&String::from_utf8_lossy(&output));
            }
            undefined()
        });
        expire(record.get_nanbox_f64());
        crate::array::js_array_set_f64(
            array_pointer(entries.get_nanbox_f64()).unwrap(),
            index,
            undefined(),
        );
        drop((encoded, output, ty));
        if let Err(error) = result {
            remember(operation.get_nanbox_f64(), error);
        }
    }
}
pub(crate) fn clear(owner: f64) {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let entries = scope.root_nanbox_f64(list(owner.get_nanbox_f64()));
    if let Some(array) = array_pointer(entries.get_nanbox_f64()) {
        let count = unsafe { (*array).length };
        for n in 0..count {
            let record =
                crate::array::js_array_get_f64(array_pointer(entries.get_nanbox_f64()).unwrap(), n);
            if object_pointer(record).is_some() {
                expire(record);
            }
        }
    }
    unsafe {
        owned_state_bag::store(
            object_pointer(owner.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_ASYNC_IMPORTS,
            undefined(),
        );
    }
}
