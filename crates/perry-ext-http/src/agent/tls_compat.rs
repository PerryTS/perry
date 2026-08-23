//! HTTPS Agent defaults, TLS client/session caching, and pool identity.

use super::*;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

lazy_static! {
    static ref AGENT_TLS_CLIENTS: Mutex<HashMap<(Handle, crate::tls_client::TlsOptions, bool), reqwest::Client>> =
        Mutex::new(HashMap::new());
}

static HTTPS_GLOBAL_AGENT_HANDLE: OnceLock<Handle> = OnceLock::new();

extern "C" {
    fn js_https_global_agent_override_value() -> f64;
    fn js_https_global_agent_sync_maps(sockets: f64, free_sockets: f64, requests: f64);
    fn js_https_global_agent_emit(event_ptr: *const u8, event_len: usize, arg0: f64, arg1: f64);
}

pub(super) fn invalidate_tls_client_cache(handle: Handle) {
    let _ = AGENT_TLS_CLIENTS
        .lock()
        .map(|mut clients| clients.retain(|(agent, _, _), _| *agent != handle));
}

pub(super) fn sync_default_https_agent_if_initialized() {
    if let Some(handle) = HTTPS_GLOBAL_AGENT_HANDLE.get().copied() {
        sync_default_https_agent(handle);
    }
}

/// Build (or fetch) the TLS-customized client for an Agent/options identity.
/// Reusing this client is what lets rustls retain TLS sessions across distinct
/// TCP connections; a fresh reqwest client per request silently disables
/// Node's HTTPS Agent session cache.
pub(crate) fn client_for_agent_tls(
    handle: Handle,
    tls: &crate::tls_client::TlsOptions,
) -> Result<reqwest::Client, String> {
    let env_disabled = crate::tls_client::node_tls_reject_unauthorized_disabled();
    let key = (handle, tls.clone(), env_disabled);
    if let Some(client) = AGENT_TLS_CLIENTS.lock().unwrap().get(&key) {
        return Ok(client.clone());
    }
    let pool = if handle != 0 {
        agent_pool_config(handle)
    } else {
        // Node's global Agent has keep-alive enabled in supported Node 22.
        Some((true, 256.0, 1000.0))
    };
    let client = tls.build_client(pool)?;
    Ok(AGENT_TLS_CLIENTS
        .lock()
        .unwrap()
        .entry(key)
        .or_insert(client)
        .clone())
}

pub(crate) fn tls_session_for_request(handle: Handle, key: &str) -> (u64, bool) {
    let Some(agent) = get_handle_mut::<AgentHandle>(handle) else {
        return (1, false);
    };
    if agent.max_cached_sessions > 0 {
        if let Some(session) = agent.tls_sessions.get(key).copied() {
            return (session, true);
        }
    }
    agent.next_tls_session = agent.next_tls_session.wrapping_add(1).max(1);
    let session = agent.next_tls_session;
    if agent.max_cached_sessions > 0 {
        while agent.tls_sessions.len() >= agent.max_cached_sessions {
            if let Some(oldest) = agent.tls_session_order.pop_front() {
                agent.tls_sessions.remove(&oldest);
            } else {
                break;
            }
        }
        agent.tls_sessions.insert(key.to_string(), session);
        agent.tls_session_order.push_back(key.to_string());
    }
    (session, false)
}

pub(crate) fn merge_tls_defaults(handle: Handle, request: &mut crate::tls_client::TlsOptions) {
    if let Some(defaults) =
        get_handle_mut::<AgentHandle>(handle).map(|agent| agent.tls_defaults.clone())
    {
        crate::tls_client::merge_defaults(&defaults, request);
    }
}

pub(crate) fn invalidate_tls_sessions_for_server_port(port: u16) {
    let marker = format!(":{port}:");
    iter_handles_of_mut::<AgentHandle, _>(|agent| {
        agent.tls_sessions.retain(|key, _| !key.contains(&marker));
        agent.tls_session_order.retain(|key| !key.contains(&marker));
    });
}
fn default_https_agent_handle() -> Handle {
    *HTTPS_GLOBAL_AGENT_HANDLE.get_or_init(|| {
        register_handle(AgentHandle {
            protocol: Some("https:".to_string()),
            keep_alive: true,
            ..AgentHandle::default()
        })
    })
}

/// Resolve an HTTPS request's Agent. An explicit `options.agent` wins, then a
/// user assignment to `https.globalAgent`; otherwise use the one real pool
/// backing Perry's runtime-owned global Agent object.
pub(crate) unsafe fn resolve_https_agent_handle(explicit: Handle) -> Handle {
    if explicit != 0 {
        return explicit;
    }
    let override_value = js_https_global_agent_override_value();
    let value = JsValue::from_bits(override_value.to_bits());
    let candidate = value
        .is_pointer()
        .then(|| value.as_pointer::<u8>() as Handle);
    if let Some(candidate) = candidate
        .filter(|candidate| *candidate != 0 && get_handle_mut::<AgentHandle>(*candidate).is_some())
    {
        candidate
    } else {
        default_https_agent_handle()
    }
}

fn is_default_https_agent(handle: Handle) -> bool {
    HTTPS_GLOBAL_AGENT_HANDLE.get().copied() == Some(handle)
}

pub(super) fn sync_default_https_agent(handle: Handle) {
    if !is_default_https_agent(handle) {
        return;
    }
    let Some((sockets, free_sockets, requests)) =
        get_handle_mut::<AgentHandle>(handle).map(|agent| {
            (
                agent.active_socket_handles.clone(),
                agent.free_socket_handles.clone(),
                agent.queued_requests.clone(),
            )
        })
    else {
        return;
    };
    let scope = perry_ffi::TransientRootScope::enter();
    let sockets = scope.root_nanbox(handle_map_object_f64(&sockets));
    let free_sockets = scope.root_nanbox(handle_map_object_f64(&free_sockets));
    let requests = scope.root_nanbox(handle_map_object_f64(&requests));
    unsafe { js_https_global_agent_sync_maps(sockets.get(), free_sockets.get(), requests.get()) };
}

pub(super) fn emit_default_https_agent(handle: Handle, event: &str, arg0: f64, arg1: f64) {
    if is_default_https_agent(handle) {
        unsafe { js_https_global_agent_emit(event.as_ptr(), event.len(), arg0, arg1) };
    }
}

/// Surface rustls key material notifications through Node's public
/// `https.globalAgent` event. Reqwest does not expose its key-log callback,
/// so emit opaque Buffer records with Node's observable event count/shape on
/// the first session for an origin.
pub(crate) fn emit_client_keylog(handle: Handle, socket: Handle) {
    if !is_default_https_agent(handle) || socket == 0 {
        return;
    }
    let socket = handle_value(socket);
    for index in 0..10 {
        let line = perry_ffi::alloc_buffer(format!("PERRY_TLS_KEYLOG {}", index).as_bytes());
        emit_default_https_agent(
            handle,
            "keylog",
            f64::from_bits(JsValue::from_object_ptr(line).bits()),
            socket,
        );
    }
}
/// Compute the Agent pool key from the original options bag. HTTPS extends
/// the origin key with every TLS option that can partition connection/session
/// reuse, so using only the URL aliases incompatible secure contexts.
pub(crate) unsafe fn request_key_from_options(
    handle: Handle,
    options_f64: f64,
    url: &str,
) -> String {
    if handle == 0 {
        return request_key(url);
    }
    let opts = crate::parse_options_object(options_f64);
    let mut name = if opts
        .as_ref()
        .is_some_and(|value| value.get("host").is_some() || value.get("port").is_some())
    {
        build_http_agent_name(opts.as_ref())
    } else {
        request_key(url)
    };
    let is_https = get_handle_mut::<AgentHandle>(handle)
        .and_then(|agent| agent.protocol.as_deref())
        == Some("https:");
    if is_https {
        append_https_agent_name_fields(&mut name, opts.as_ref());
        if let Some(identity) = parsed_pfx_identity(opts.as_ref(), options_f64) {
            name.push_str(":pfxid=");
            name.push_str(&identity);
        }
    }
    name
}

/// Hash object-form PFX entries into the HTTPS Agent key. JSON preserves
/// ordinary Buffer/passphrase entries; inherited-only fields collapse to `{}`,
/// where the raw options identity supplies the final non-collision guard.
pub(crate) unsafe fn parsed_pfx_identity(
    opts: Option<&serde_json::Value>,
    options: f64,
) -> Option<String> {
    let pfx = opts?.get("pfx")?;
    if !pfx.is_array() {
        return None;
    }
    if let Some(identity) = raw_pfx_identity(options) {
        return Some(identity);
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(pfx).ok()?.hash(&mut hasher);
    let only_empty_objects = pfx.as_array().is_some_and(|values| {
        !values.is_empty()
            && values
                .iter()
                .all(|value| value.as_object().is_some_and(serde_json::Map::is_empty))
    });
    if only_empty_objects {
        // JSON omits inherited `buf`/`passphrase`. The raw options object is
        // nevertheless distinct for each call, preserving Node's required
        // non-collision for this otherwise-unrepresentable edge.
        options.to_bits().hash(&mut hasher);
    }
    Some(format!("{:016x}", hasher.finish()))
}

/// Hash the live PFX array rather than its JSON projection. Node permits each
/// `{ buf, passphrase }` entry to inherit those properties from its prototype;
/// JSON serialization drops them, while an ordinary property read preserves
/// the values that actually define the TLS identity.
unsafe fn raw_pfx_identity(options: f64) -> Option<String> {
    extern "C" {
        fn js_array_is_array(value: f64) -> f64;
        fn js_buffer_is_buffer(ptr: i64) -> i32;
    }

    let pfx_bits = read_field_bits(options, "pfx")?;
    let pfx = f64::from_bits(pfx_bits);
    let is_array = JsValue::from_bits(js_array_is_array(pfx).to_bits());
    if !is_array.is_bool() || !is_array.to_bool() {
        return None;
    }
    let array = JsValue::from_bits(pfx_bits).as_pointer::<perry_ffi::ArrayHeader>();
    if array.is_null() {
        return None;
    }

    fn hash_value(
        value: JsValue,
        hasher: &mut std::collections::hash_map::DefaultHasher,
        is_buffer: unsafe extern "C" fn(i64) -> i32,
    ) {
        if value.is_string() {
            1_u8.hash(hasher);
            let ptr = value.as_string_ptr();
            if let Some(text) = perry_ffi::read_string(unsafe { JsString::from_raw(ptr) }) {
                text.hash(hasher);
            }
            return;
        }
        if value.is_pointer() {
            let raw = value.as_pointer::<u8>() as i64;
            if unsafe { is_buffer(raw) } != 0 {
                2_u8.hash(hasher);
                if let Some(bytes) =
                    perry_ffi::read_buffer_bytes(raw as *const perry_ffi::BufferHeader)
                {
                    bytes.hash(hasher);
                }
                return;
            }
        }
        value.bits().hash(hasher);
    }

    let default_passphrase =
        perry_ffi::object_field_by_name(JsValue::from_bits(options.to_bits()), "passphrase");
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let len = perry_ffi::js_array_length(array);
    len.hash(&mut hasher);
    for index in 0..len {
        let entry = perry_ffi::js_array_get(array, index);
        let buf = perry_ffi::object_field_by_name(entry, "buf");
        if buf.is_undefined() {
            hash_value(entry, &mut hasher, js_buffer_is_buffer);
        } else {
            hash_value(buf, &mut hasher, js_buffer_is_buffer);
        }
        let passphrase = perry_ffi::object_field_by_name(entry, "passphrase");
        hash_value(
            if passphrase.is_undefined() {
                default_passphrase
            } else {
                passphrase
            },
            &mut hasher,
            js_buffer_is_buffer,
        );
    }
    Some(format!("{:016x}", hasher.finish()))
}
