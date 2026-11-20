//! Agent-local wrapper adapters and shared transfer-list admission.
use super::port_transport::{self, Lease};
use crate::feature_hooks::Hook;
use crate::value::JSValue;
static LOOKUP: Hook<fn(f64) -> Option<Lease>> = Hook::empty();
static ADOPT: Hook<fn(Lease) -> f64> = Hook::empty();
pub fn register(lookup: fn(f64) -> Option<Lease>, adopt: fn(Lease) -> f64) {
    LOOKUP.set(lookup);
    ADOPT.set(adopt);
}
pub fn lookup(value: f64) -> Option<Lease> {
    match super::message_port_id(value)? {
        0 => super::runtime_port_lease(value),
        _ => LOOKUP.peek().and_then(|lookup| lookup(value)),
    }
}
pub fn adopt(lease: Lease) -> f64 {
    ADOPT
        .peek()
        .map_or_else(|| super::adopt_runtime_port(lease), |adopt| adopt(lease))
}
#[derive(Default)]
pub struct Prepared {
    pub buffers: Vec<usize>,
    pub ports: Vec<Lease>,
}
pub fn prepare(list: f64, source: Option<Lease>) -> Result<Prepared, String> {
    const INVALID: &str = "Found invalid value in transferList.";
    let scope = crate::gc::RuntimeHandleScope::new();
    let list = scope.root_nanbox_f64(list);
    let value = JSValue::from_bits(list.get_nanbox_u64());
    if value.is_undefined() {
        return Ok(Prepared::default());
    }
    let addr = crate::value::js_nanbox_get_pointer(list.get_nanbox_f64()) as usize;
    if !value.is_pointer()
        || crate::array::js_array_is_array(list.get_nanbox_f64()).to_bits()
            != JSValue::bool(true).bits()
    {
        return Err(INVALID.into());
    }
    let len = crate::array::js_array_length(addr as *const crate::array::ArrayHeader);
    // Collect every observable array index before semantic admission.
    let entries: Vec<_> = (0..len)
        .map(|index| {
            let array = crate::value::js_nanbox_get_pointer(list.get_nanbox_f64())
                as *const crate::array::ArrayHeader;
            scope.root_nanbox_f64(crate::array::js_array_get_f64(array, index))
        })
        .collect();
    let mut prepared = Prepared::default();
    for entry in entries {
        let value = entry.get_nanbox_f64();
        if let Some(lease) = lookup(value) {
            if source == Some(lease) {
                return Err("Transfer list contains source port".into());
            }
            if !port_transport::alive(lease) {
                return Err("MessagePort in transfer list is already detached".into());
            }
            if prepared.ports.contains(&lease) {
                return Err("Transfer list contains duplicate MessagePort".into());
            }
            if crate::clone_markers::is_untransferable(value) {
                return Err("Cannot transfer object of unsupported type.".into());
            }
            prepared.ports.push(lease);
        } else {
            if super::message_port_id(value).is_some() {
                return Err("MessagePort in transfer list is already detached".into());
            }
            if !JSValue::from_bits(value.to_bits()).is_pointer() {
                return Err(INVALID.into());
            }
            let address = crate::value::js_nanbox_get_pointer(value) as usize;
            crate::thread::validate_message_buffer(address, &prepared.buffers)?;
            prepared.buffers.push(address);
        }
    }
    Ok(prepared)
}

/// Root admission and graph traversal together; thrown getter values survive unchanged.
pub fn serialize(
    value: f64,
    transfer: f64,
    source: Option<Lease>,
    admit: impl FnOnce(f64) -> f64,
) -> Result<crate::thread::SerializedValue, String> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let transfer = scope.root_nanbox_f64(transfer);
    let mut prepared = Prepared::default();
    let outcome = crate::exception::catch_thrown(|| {
        let list = admit(transfer.get_nanbox_f64());
        prepared = prepare(list, source)?;
        unsafe {
            crate::thread::serialize_message_with_ports(
                value.get_nanbox_u64(),
                &prepared.buffers,
                &prepared.ports,
                source,
                None,
            )
        }
    });
    drop(prepared);
    drop(scope);
    match outcome {
        Ok(result) => result,
        Err(error) => crate::exception::rethrow_caught(error),
    }
}

pub fn send(lease: Lease, message: crate::thread::SerializedValue) {
    if port_transport::send(lease, message) {
        let warning =
            "The target port was posted to itself, and the communication channel was lost";
        let warning = crate::string::js_string_from_bytes(warning.as_ptr(), warning.len() as u32);
        let undefined = f64::from_bits(JSValue::undefined().bits());
        crate::process::js_process_emit_warning(
            crate::value::js_nanbox_string(warning as i64),
            undefined,
            undefined,
        );
    }
}

pub fn throw_data_clone_error(message: &str) -> ! {
    // The runtime throw uses longjmp, so no Rust guard can release roots from
    // the abandoned traversal. They are no longer needed once cloning fails.
    let scope = crate::gc::RuntimeHandleScope::new();
    let message = scope.root_nanbox_f64(crate::value::js_nanbox_string(
        crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32) as i64,
    ));
    let name = scope.root_nanbox_f64(crate::value::js_nanbox_string(
        crate::string::js_string_from_bytes(b"DataCloneError".as_ptr(), 14) as i64,
    ));
    let err = crate::event_target_hook_slots::js_dom_exception_new(
        message.get_nanbox_f64(),
        name.get_nanbox_f64(),
    );
    crate::exception::throw_open(crate::value::js_nanbox_pointer(err as i64))
}
