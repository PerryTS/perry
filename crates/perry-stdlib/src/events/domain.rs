//! node:domain integration surface for EventEmitter handles.
//!
//! Bridges the per-emitter `domain_handle` slot (set when an emitter is
//! created or `.add()`-ed while a domain is active) to the C-ABI accessors
//! that perry-codegen and the stdlib domain dispatch reach for. Kept in a
//! sibling module so `events.rs` stays under the 2000-line ceiling.

use super::{EventEmitterHandle, TAG_NULL_F64_BITS};
use crate::common::{get_handle, get_handle_mut, Handle};
use perry_runtime::js_nanbox_pointer;

pub fn is_event_emitter_handle(handle: Handle) -> bool {
    get_handle::<EventEmitterHandle>(handle).is_some()
}

#[no_mangle]
pub extern "C" fn js_event_emitter_set_domain(handle: Handle, domain: Handle) -> i32 {
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.domain_handle = if domain == 0 { None } else { Some(domain) };
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn js_event_emitter_get_domain(handle: Handle) -> Handle {
    get_handle::<EventEmitterHandle>(handle)
        .and_then(|emitter| emitter.domain_handle)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn js_event_emitter_domain_value(handle: Handle) -> f64 {
    let domain = js_event_emitter_get_domain(handle);
    if domain == 0 {
        f64::from_bits(TAG_NULL_F64_BITS)
    } else {
        js_nanbox_pointer(domain)
    }
}
