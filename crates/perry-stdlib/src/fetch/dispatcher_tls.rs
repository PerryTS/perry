//! Per-request trust settings carried by an undici `Agent` passed as
//! `fetch(url, { dispatcher })`.
//!
//! perry-ext-undici reads the Agent's `connect` options when it constructs the
//! Agent and registers them here under the Agent's handle id; `Request`
//! construction resolves `init.dispatcher` back to a TLS profile.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::turnloop_tls_client::{profile_for, TlsOverride, TlsProfile};

static DISPATCHER_TLS: Mutex<Option<HashMap<usize, TlsOverride>>> = Mutex::new(None);

/// Record (or, with an empty override, clear) the TLS settings of the
/// dispatcher with handle id `handle`. `reject_unauthorized` is `-1` when the
/// option was absent, otherwise `0` or `1`. Every dispatcher constructor calls
/// this, so a recycled handle id never inherits a previous Agent's settings.
///
/// # Safety
/// `ca_pem_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_fetch_set_dispatcher_tls(
    handle: i64,
    ca_pem_ptr: *const perry_runtime::StringHeader,
    reject_unauthorized: i32,
) {
    let over = TlsOverride {
        ca_pem: super::string_from_header(ca_pem_ptr)
            .map(String::into_bytes)
            .unwrap_or_default(),
        reject_unauthorized: match reject_unauthorized {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        },
    };
    let Ok(mut guard) = DISPATCHER_TLS.lock() else {
        return;
    };
    let map = guard.get_or_insert_with(Default::default);
    if over == TlsOverride::default() {
        map.remove(&(handle as usize));
    } else {
        map.insert(handle as usize, over);
    }
}

/// The TLS profile of the dispatcher JS value `dispatcher`, when it is an Agent
/// registered with non-default `connect` options.
pub(super) fn tls_for_dispatcher(dispatcher: f64) -> Option<&'static TlsProfile> {
    let id = super::handle_id(dispatcher);
    if id == 0 {
        return None;
    }
    let over = DISPATCHER_TLS.lock().ok()?.as_ref()?.get(&id)?.clone();
    profile_for(&over)
}
