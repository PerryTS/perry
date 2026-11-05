//! An invocation owns this ordinary traced array independently of its public
//! Promise. Native metadata stores public serial IDs, never untraced GC tokens.
use super::*;
use crate::{async_context as als, async_hooks::AsyncResourceIds};

pub(in crate::component) fn capture(promise: f64) -> (f64, AsyncResourceIds) {
    let scope = RuntimeHandleScope::new();
    let promise = scope.root_nanbox_f64(promise);
    let pointer = JSValue::from_bits(promise.get_nanbox_f64().to_bits())
        .as_pointer::<crate::promise::Promise>();
    let native = unsafe {
        AsyncResourceIds {
            async_id: (*pointer).async_id,
            trigger_async_id: (*pointer).trigger_async_id,
        }
    };
    let metadata = scope.root_nanbox_f64(
        if crate::async_hooks::resource_state::get(native.async_id).is_some() {
            crate::value::js_nanbox_pointer(native.async_id as i64)
        } else {
            undefined()
        },
    );
    let snapshot = scope.root_raw_mut_ptr(als::alloc_snapshot_holder(
        crate::promise::context_for_promise(pointer.cast_mut()),
    ));
    let record = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(2));
    crate::array::js_array_set_f64(
        record.get_raw_mut_ptr(),
        0,
        crate::value::js_nanbox_pointer(
            snapshot.get_raw_mut_ptr::<als::AsyncSnapshotHolder>() as i64
        ),
    );
    crate::array::js_array_set_f64(record.get_raw_mut_ptr(), 1, metadata.get_nanbox_f64());
    (
        crate::value::js_nanbox_pointer(
            record.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64
        ),
        AsyncResourceIds {
            async_id: crate::async_hooks::resource_state::public_id(native.async_id),
            trigger_async_id: native.trigger_async_id,
        },
    )
}

/// Initialization and synchronous submissions have a snapshot without a public
/// Promise. Origin zero preserves the execution scope of the embedding caller.
pub(in crate::component) fn current() -> f64 {
    let scope = RuntimeHandleScope::new();
    let snapshot = scope.root_raw_mut_ptr(als::alloc_snapshot_holder(als::capture_context()));
    let record = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(2));
    crate::array::js_array_set_f64(
        record.get_raw_mut_ptr(),
        0,
        crate::value::js_nanbox_pointer(
            snapshot.get_raw_mut_ptr::<als::AsyncSnapshotHolder>() as i64
        ),
    );
    crate::array::js_array_set_f64(record.get_raw_mut_ptr(), 1, undefined());
    crate::value::js_nanbox_pointer(record.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64)
}
