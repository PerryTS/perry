//! The one structured clone behind every `postMessage`: Worker,
//! `parentPort`, MessagePort, BroadcastChannel, `postMessageToThread` and
//! `workerData`. The value walk lives in `perry_runtime::thread`; this file
//! reads the transfer list and the objects marked uncloneable.

use super::*;

/// Clone `value` as Node's `postMessage(value, transfer)` does. `transfer` is
/// an iterable, options containing one, or undefined. ArrayBuffers in it are
/// detached once the clone succeeded; their native backing ownership moves
/// through the queued SerializedValue to the receiving heap. Throws `DataCloneError` on failure.
pub(super) fn clone_message(value: f64, transfer: f64) -> SerializedValue {
    match try_clone_message(value, transfer) {
        Ok(message) => message,
        // Thrown only here, after every handle scope above was dropped.
        Err(detail) => throw_data_clone_error(&detail),
    }
}

pub(super) fn try_clone_message(value: f64, transfer: f64) -> Result<SerializedValue, String> {
    clone_with_admission(value, transfer, None, |transfer| {
        perry_runtime::thread::js_worker_transfer_sequence(transfer)
    })
}

pub(super) fn clone_message_from_port(value: f64, transfer: f64, source: Lease) -> SerializedValue {
    match clone_with_admission(value, transfer, Some(source), |transfer| {
        perry_runtime::thread::js_worker_transfer_sequence(transfer)
    }) {
        Ok(message) => message,
        Err(detail) => throw_data_clone_error(&detail),
    }
}

pub(super) fn clone_worker_data(value: f64, transfer: f64) -> SerializedValue {
    match clone_with_admission(value, transfer, None, legacy_constructor_transfer) {
        Ok(message) => message,
        Err(detail) => throw_data_clone_error(&detail),
    }
}

fn legacy_constructor_transfer(transfer: f64) -> f64 {
    if array_ptr_from_value(transfer).is_some() {
        transfer
    } else if object_ptr_from_value(transfer).is_some() {
        get_object_field_from_value(transfer, "transfer")
    } else {
        js_undefined()
    }
}

fn clone_with_admission(
    value: f64,
    transfer: f64,
    source: Option<Lease>,
    admit: impl FnOnce(f64) -> f64,
) -> Result<SerializedValue, String> {
    perry_runtime::messaging::port_transfer::serialize(value, transfer, source, admit)
}

#[cfg(test)]
mod transfer_tests {
    use super::*;

    #[test]
    fn worker_data_adopts_transfer_once_and_roots_repeated_reads() {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let source = perry_runtime::buffer::js_array_buffer_new(128 * 1024);
        let original = perry_runtime::buffer::bytes::no_gc(|scope| {
            perry_runtime::buffer::bytes::bytes(
                perry_runtime::value::js_nanbox_pointer(source as i64),
                scope,
            )
            .unwrap()
            .as_ptr() as usize
        });
        let message = unsafe {
            perry_runtime::thread::serialize_message(
                JSValue::pointer(source.cast()).bits(),
                &[source as usize],
                None,
            )
            .unwrap()
        };
        let received = unsafe { deserialize_nanbox_on_current_thread(&message) };
        CURRENT_WORKER_DATA.with(|slot| *slot.borrow_mut() = Some(received));
        let first = js_worker_threads_get_worker_data();
        let root = scope.root_nanbox_f64(first);
        perry_runtime::gc::js_gc_collect();
        let second = js_worker_threads_get_worker_data();
        assert_eq!(root.get_nanbox_f64().to_bits(), second.to_bits());
        let buffer = perry_runtime::value::js_nanbox_get_pointer(second)
            as *const perry_runtime::buffer::BufferHeader;
        perry_runtime::buffer::bytes::no_gc(|scope| {
            assert_eq!(
                perry_runtime::buffer::bytes::bytes(
                    perry_runtime::value::js_nanbox_pointer(buffer as i64),
                    scope
                )
                .unwrap()
                .as_ptr() as usize,
                original
            )
        });
        assert_eq!(
            perry_runtime::buffer::js_buffer_length(buffer) as u32,
            128 * 1024
        );
        CURRENT_WORKER_DATA.with(|slot| *slot.borrow_mut() = None);
    }
}
