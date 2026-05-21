//! JS handle table + native promise resolver storage.

use super::*;

/// Store a V8 value in the handle table and return a handle ID
pub fn store_js_handle(scope: &mut v8::PinScope<'_, '_>, value: v8::Local<v8::Value>) -> u64 {
    let handle_id = NEXT_HANDLE_ID.with(|id| {
        let current = id.get();
        id.set(current + 1);
        current
    });
    let global = v8::Global::new(scope, value);
    JS_OBJECT_HANDLES.with(|handles| {
        handles.borrow_mut().insert(handle_id, global);
    });
    bump_js_handle_stored();
    handle_id
}

/// Retrieve a V8 value from the handle table
pub fn get_js_handle<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    handle: u64,
) -> Option<v8::Local<'s, v8::Value>> {
    JS_OBJECT_HANDLES.with(|handles| {
        handles
            .borrow()
            .get(&handle)
            .map(|g| v8::Local::new(scope, g))
    })
}

/// Release a V8 handle from the table
pub fn release_js_handle(handle: u64) -> bool {
    let released = JS_OBJECT_HANDLES.with(|handles| handles.borrow_mut().remove(&handle).is_some());
    if released {
        bump_js_handle_released();
    }
    released
}

/// Check if a NaN-boxed value is a JS handle
pub fn is_js_handle(value: f64) -> bool {
    let bits = value.to_bits();
    (bits & TAG_MASK) == JS_HANDLE_TAG
}

/// Extract handle ID from a NaN-boxed JS handle value
pub fn get_handle_id(value: f64) -> Option<u64> {
    let bits = value.to_bits();
    if (bits & TAG_MASK) == JS_HANDLE_TAG {
        Some(bits & POINTER_MASK)
    } else {
        None
    }
}

/// Create a NaN-boxed value representing a JS handle
pub fn make_js_handle_value(handle_id: u64) -> f64 {
    f64::from_bits(JS_HANDLE_TAG | (handle_id & POINTER_MASK))
}

pub(super) fn store_native_promise_resolver(
    scope: &mut v8::PinScope<'_, '_>,
    resolver: v8::Local<v8::PromiseResolver>,
) -> u64 {
    let resolver_id = NEXT_NATIVE_PROMISE_RESOLVER_ID.with(|id| {
        let current = id.get();
        id.set(current + 1);
        current
    });
    NATIVE_PROMISE_RESOLVERS.with(|resolvers| {
        resolvers
            .borrow_mut()
            .insert(resolver_id, v8::Global::new(scope, resolver));
    });
    resolver_id
}

pub(super) fn take_native_promise_resolver<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    resolver_id: u64,
) -> Option<v8::Local<'s, v8::PromiseResolver>> {
    NATIVE_PROMISE_RESOLVERS.with(|resolvers| {
        resolvers
            .borrow_mut()
            .remove(&resolver_id)
            .map(|resolver| v8::Local::new(scope, resolver))
    })
}
