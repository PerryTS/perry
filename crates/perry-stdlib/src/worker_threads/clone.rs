//! The one structured clone behind every `postMessage`: Worker,
//! `parentPort`, MessagePort, BroadcastChannel, `postMessageToThread` and
//! `workerData`. The value walk lives in `perry_runtime::thread`; this file
//! reads the transfer list and the objects marked uncloneable.

use super::*;

/// Clone `value` as Node's `postMessage(value, transfer)` does. `transfer` is
/// an array, `{ transfer: [...] }`, or undefined. ArrayBuffers in it are
/// detached once the clone succeeded. Throws `DataCloneError` on failure.
pub(super) fn clone_message(value: f64, transfer: f64) -> SerializedValue {
    match try_clone_message(value, transfer) {
        Ok(message) => message,
        // Thrown only here, after every handle scope above was dropped.
        Err(detail) => throw_data_clone_error(&detail),
    }
}

pub(super) fn try_clone_message(value: f64, transfer: f64) -> Result<SerializedValue, String> {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    // Reading the transfer list can allocate.
    let value = scope.root_nanbox_f64(value);
    let buffers = transfer_buffers(transfer)?;
    let marked: Vec<u64> = UNCLONEABLE_OBJECTS.with(|set| set.borrow().iter().copied().collect());
    let refuse = |bits: u64| {
        // Node ignores markAsUncloneable on an ArrayBuffer.
        marked.contains(&bits)
            && !perry_runtime::buffer::is_array_buffer((bits & 0x0000_FFFF_FFFF_FFFF) as usize)
    };
    let uncloneable: Option<&dyn Fn(u64) -> bool> = if marked.is_empty() {
        None
    } else {
        Some(&refuse)
    };
    unsafe {
        perry_runtime::thread::serialize_message(
            value.get_nanbox_f64().to_bits(),
            &buffers,
            uncloneable,
        )
    }
}

/// The ArrayBuffer addresses named by a transfer list. MessagePorts are
/// skipped: moving a port to another thread is not supported, and a port in
/// the list was always ignored. Other entries go through, and the runtime
/// rejects what is not an ArrayBuffer.
fn transfer_buffers(transfer: f64) -> Result<Vec<usize>, String> {
    const INVALID: &str = "Found invalid value in transferList.";
    let list = if array_ptr_from_value(transfer).is_some() {
        transfer
    } else if object_ptr_from_value(transfer).is_some() {
        get_object_field_from_value(transfer, "transfer")
    } else {
        return Ok(Vec::new());
    };
    if array_ptr_from_value(list).is_none() {
        return if is_undefined(list) {
            Ok(Vec::new())
        } else {
            Err(INVALID.to_string())
        };
    }
    // The port check below allocates, so the list is re-read from a root.
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let list = scope.root_nanbox_f64(list);
    let array = || array_ptr_from_value(list.get_nanbox_f64()).unwrap();
    let len = perry_runtime::array::js_array_length(array());
    let mut buffers = Vec::with_capacity(len as usize);
    for index in 0..len {
        let item = perry_runtime::array::js_array_get_f64(array(), index);
        if !JSValue::from_bits(item.to_bits()).is_pointer() {
            return Err(INVALID.to_string());
        }
        // Buffers do not move, so their addresses stay valid.
        let addr = perry_runtime::value::js_nanbox_get_pointer(item) as usize;
        if perry_runtime::buffer::is_array_buffer(addr) {
            if UNTRANSFERABLE_OBJECTS.with(|set| set.borrow().contains(&item.to_bits())) {
                return Err("An ArrayBuffer is marked as untransferable.".to_string());
            }
            buffers.push(addr);
        } else if object_ptr_from_value(item).is_none() || port_id_from_object(item).is_none() {
            buffers.push(addr);
        }
    }
    Ok(buffers)
}
