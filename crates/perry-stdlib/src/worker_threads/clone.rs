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
    clone_with_admission(value, transfer, |transfer| {
        perry_runtime::thread::js_worker_transfer_sequence(transfer)
    })
}

pub(super) fn clone_worker_data(value: f64, transfer: f64) -> SerializedValue {
    match clone_with_admission(value, transfer, legacy_constructor_transfer) {
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
    admit: impl FnOnce(f64) -> f64,
) -> Result<SerializedValue, String> {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let transfer = scope.root_nanbox_f64(transfer);
    // JS throws abandon native frames; ownership stays outside the trap.
    let mut buffers = Vec::new();
    let outcome = perry_runtime::exception::catch_js_throw(|| {
        let list = admit(transfer.get_nanbox_f64());
        transfer_buffers(list, &mut buffers)?;
        unsafe {
            perry_runtime::thread::serialize_message(
                value.get_nanbox_f64().to_bits(),
                &buffers,
                None,
            )
        }
    });
    drop(buffers);
    drop(scope);
    match outcome {
        Ok(message) => message,
        Err(error) => perry_runtime::exception::js_throw(error),
    }
}

/// The ArrayBuffer addresses named by a transfer list. MessagePorts are
/// skipped: moving a port to another thread is not supported, and a port in
/// the list was always ignored. Other entries go through, and the runtime
/// rejects what is not an ArrayBuffer.
fn transfer_buffers(list: f64, buffers: &mut Vec<usize>) -> Result<(), String> {
    const INVALID: &str = "Found invalid value in transferList.";
    if array_ptr_from_value(list).is_none() {
        return if is_undefined(list) {
            Ok(())
        } else {
            Err(INVALID.to_string())
        };
    }
    // The port check below allocates, so the list is re-read from a root.
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let list = scope.root_nanbox_f64(list);
    let array = || array_ptr_from_value(list.get_nanbox_f64()).unwrap();
    let len = perry_runtime::array::js_array_length(array());
    buffers.reserve(len as usize);
    for index in 0..len {
        let item_scope = perry_runtime::gc::RuntimeHandleScope::new();
        let item =
            item_scope.root_nanbox_f64(perry_runtime::array::js_array_get_f64(array(), index));
        let item_value = || item.get_nanbox_f64();
        if !JSValue::from_bits(item_value().to_bits()).is_pointer() {
            return Err(INVALID.to_string());
        }
        // Buffers do not move, so their addresses stay valid.
        let addr = perry_runtime::value::js_nanbox_get_pointer(item_value()) as usize;
        if perry_runtime::clone_markers::is_untransferable(item_value()) {
            return Err("An object is marked as untransferable.".to_string());
        }
        if perry_runtime::buffer::is_array_buffer(addr) {
            buffers.push(addr);
        } else if object_ptr_from_value(item_value()).is_none()
            || port_id_from_object(item_value()).is_none()
        {
            buffers.push(perry_runtime::value::js_nanbox_get_pointer(item_value()) as usize);
        }
    }
    Ok(())
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
