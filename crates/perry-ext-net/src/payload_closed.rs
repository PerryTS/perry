//! Pending terminal callback context is owned by the ordinary JS owner.
//!
//! Explicit release destroys Rust state immediately. Its later Closed still
//! needs the error and server edge from that call even when the owner has
//! reopened. These are ordinary callback records, matched by the driver's
//! existing generational capability; they never route or pin a native cell.

use super::payload_transport as p;
use perry_ffi::{ArrayHeader, JsValue, TransientRootScope};

extern "C" {
    fn js_object_alloc_null_proto(class_id: u32, fields: u32) -> *mut perry_ffi::ObjectHeader;
}

fn array_ptr(value: f64) -> *mut ArrayHeader {
    JsValue::from_bits(value.to_bits()).as_pointer()
}

pub(crate) fn parts(record: f64) -> [u32; 4] {
    let scope = TransientRootScope::enter();
    let record = scope.root_nanbox(record);
    let handle = scope.root_nanbox(p::own_get(record.get(), "handle"));
    std::array::from_fn(|index| unsafe {
        perry_ffi::js_array_get(array_ptr(handle.get()), index as u32).to_number() as u32
    })
}

pub(crate) fn append(state: f64, record: f64) {
    let scope = TransientRootScope::enter();
    let state = scope.root_nanbox(state);
    let record = scope.root_nanbox(record);
    let mut array = scope.root_nanbox(p::own_get(state.get(), "closeCallbacks"));
    if !JsValue::from_bits(array.get().to_bits()).is_pointer() {
        array = scope.root_nanbox(p::boxed_addr(unsafe { perry_ffi::js_array_alloc(0) } as i64));
    }
    let updated = unsafe {
        perry_ffi::js_array_push(
            array_ptr(array.get()),
            JsValue::from_bits(record.get().to_bits()),
        )
    };
    p::own_set(state.get(), "closeCallbacks", p::boxed_addr(updated as i64));
}

pub(crate) fn record(parts: [u32; 4]) -> f64 {
    let scope = TransientRootScope::enter();
    let record = scope.root_nanbox(p::boxed_addr(
        unsafe { js_object_alloc_null_proto(0, 4) } as i64
    ));
    let mut capability = scope.root_addr(unsafe { perry_ffi::js_array_alloc(4) } as i64);
    for part in parts {
        capability = scope.root_addr(unsafe {
            perry_ffi::js_array_push(
                capability.get() as *mut ArrayHeader,
                JsValue::from_number(part as f64),
            )
        } as i64);
    }
    p::own_set(record.get(), "handle", p::boxed_addr(capability.get()));
    record.get()
}

pub(crate) fn peek(state: f64, capability: [u32; 4]) -> f64 {
    let scope = TransientRootScope::enter();
    let state = scope.root_nanbox(state);
    let array = scope.root_nanbox(p::own_get(state.get(), "closeCallbacks"));
    if !JsValue::from_bits(array.get().to_bits()).is_pointer() {
        return p::undefined();
    }
    let count = unsafe { perry_ffi::js_array_length(array_ptr(array.get())) };
    for index in 0..count {
        let record = scope.root_nanbox(f64::from_bits(unsafe {
            perry_ffi::js_array_get(array_ptr(array.get()), index).bits()
        }));
        if parts(record.get()) == capability {
            return record.get();
        }
    }
    p::undefined()
}

pub(crate) fn for_wrapper(state: f64, owner: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let state = scope.root_nanbox(state);
    let owner = scope.root_nanbox(owner);
    let array = scope.root_nanbox(p::own_get(state.get(), "closeCallbacks"));
    if !JsValue::from_bits(array.get().to_bits()).is_pointer() {
        return p::undefined();
    }
    let count = unsafe { perry_ffi::js_array_length(array_ptr(array.get())) };
    for index in 0..count {
        let record = scope.root_nanbox(f64::from_bits(unsafe {
            perry_ffi::js_array_get(array_ptr(array.get()), index).bits()
        }));
        if p::own_get(record.get(), "tlsWrapper").to_bits() == owner.get().to_bits() {
            return record.get();
        }
    }
    p::undefined()
}

/// Consume exactly this handle's terminal callback record. All other records
/// keep their ordinary GC edges; completions across handles may arrive in
/// either order. There is no FIFO assumption across incarnations.
pub(crate) fn take(state: f64, parts: [u32; 4]) -> f64 {
    let scope = TransientRootScope::enter();
    let state = scope.root_nanbox(state);
    let array = scope.root_nanbox(p::own_get(state.get(), "closeCallbacks"));
    if !JsValue::from_bits(array.get().to_bits()).is_pointer() {
        return p::undefined();
    }
    let count = unsafe { perry_ffi::js_array_length(array_ptr(array.get())) };
    let records: Vec<_> = (0..count)
        .map(|index| {
            scope.root_nanbox(f64::from_bits(unsafe {
                perry_ffi::js_array_get(array_ptr(array.get()), index).bits()
            }))
        })
        .collect();
    let found = records.iter().position(|record| {
        let handle = scope.root_nanbox(p::own_get(record.get(), "handle"));
        if !JsValue::from_bits(handle.get().to_bits()).is_pointer() {
            return false;
        }
        (0..4).all(|index| unsafe {
            perry_ffi::js_array_get(array_ptr(handle.get()), index).to_number()
                == parts[index as usize] as f64
        })
    });
    let Some(found) = found else {
        return p::undefined();
    };
    let mut kept =
        scope.root_addr(unsafe { perry_ffi::js_array_alloc(count.saturating_sub(1)) } as i64);
    for (index, record) in records.iter().enumerate() {
        if index != found {
            kept = scope.root_addr(unsafe {
                perry_ffi::js_array_push(
                    kept.get() as *mut ArrayHeader,
                    JsValue::from_bits(record.get().to_bits()),
                )
            } as i64);
        }
    }
    p::own_set(state.get(), "closeCallbacks", p::boxed_addr(kept.get()));
    records[found].get()
}
