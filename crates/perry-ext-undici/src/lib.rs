//! Native bindings for the npm `undici` package — uses only `perry-ffi`.
//!
//! Perry already ships a native Web Fetch stack (perry-stdlib `fetch/`,
//! mirrored by `perry-ext-fetch`), so this crate is thin glue rather than
//! an AOT compile of undici's ~50k lines of JS:
//!
//! - `new ProxyAgent(uri | { uri, token? })` stores the proxy config in a
//!   perry-ffi handle.
//! - `setGlobalDispatcher(agent)` pushes that config into the native fetch
//!   client via the `js_fetch_set_global_proxy` symbol (defined by BOTH
//!   perry-stdlib's fetch and perry-ext-fetch; whichever copy is linked
//!   wins — see `prefer_well_known_before_stdlib`). From then on every
//!   `fetch()` — global or imported from `undici` — tunnels through the
//!   proxy (reqwest performs HTTP CONNECT for https targets, absolute-form
//!   proxying for http, and sends the token as `Proxy-Authorization`).
//! - `new Agent(...)` is a direct-connection dispatcher; installing it
//!   clears any proxy.
//! - `fetch` from `'undici'` is served by perry's native fetch (the
//!   codegen lowers it to the same `js_fetch_*` symbols as global fetch).
//! - `getGlobalDispatcher()` returns the installed dispatcher handle
//!   (creating a default `Agent` on first call, matching undici).
//! - `request()` is NOT implemented — it rejects with a clear error
//!   pointing at `fetch`. undici's streams/pools/mock surface is likewise
//!   out of scope for this binding.
//!
//! # Version compatibility
//!
//! Targets undici's stable dispatcher API: `ProxyAgent`, `Agent`,
//! `setGlobalDispatcher`, `getGlobalDispatcher`, and `fetch` are unchanged
//! across undici 6.x → 7.x, so the binding tracks both majors.
//!
//! # Known limitations
//!
//! - `ProxyAgent` options other than `uri`/`token` (`requestTls`,
//!   `proxyTls`, `clientFactory`, connection-pool tuning) are accepted but
//!   ignored — the native reqwest client owns TLS and pooling.
//! - `agent.close()`/`agent.destroy()` resolve immediately; the native
//!   client's connection pool is process-global and is not torn down.

use perry_ffi::{
    read_string, register_handle, throw_with_code, with_handle, ErrorKind, Handle, JsPromise,
    JsString, Promise, StringHeader,
};
use std::sync::Mutex;

/// Proxy configuration parsed from `new ProxyAgent(...)` options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyConfig {
    /// Proxy origin, e.g. `http://127.0.0.1:8080`.
    pub uri: String,
    /// Literal `Proxy-Authorization` header value, e.g. `Basic <base64>`.
    pub token: Option<String>,
}

/// A dispatcher handle — what `setGlobalDispatcher` accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dispatcher {
    /// `new Agent(...)` — direct connections (no proxy).
    Agent,
    /// `new ProxyAgent(...)` — route through an HTTP(S) proxy.
    ProxyAgent(ProxyConfig),
}

/// The handle most recently installed via `setGlobalDispatcher`, plus the
/// lazily created default `Agent` that `getGlobalDispatcher()` returns
/// before anything was installed (mirrors undici's implicit global Agent).
static GLOBAL_DISPATCHER: Mutex<Option<Handle>> = Mutex::new(None);

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let h = JsString::from_raw(ptr as *mut StringHeader);
    read_string(h).map(String::from)
}

/// Parse `new ProxyAgent(opts)` options. The codegen coerces the argument
/// with `js_value_to_str_ptr_for_ffi`, so `raw` is either the URI string
/// itself (`new ProxyAgent('http://localhost:8080')`) or the options
/// object JSON-stringified (`new ProxyAgent({ uri, token })`).
///
/// Exposed (not part of the FFI surface) so unit tests can pin the parse
/// behavior without a runtime.
pub fn parse_proxy_agent_options(raw: &str) -> Result<ProxyConfig, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("ProxyAgent: expected a proxy uri or an options object".to_string());
    }
    let (uri, token) = if trimmed.starts_with('{') {
        let value: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|e| format!("ProxyAgent: invalid options object: {e}"))?;
        let uri = value
            .get("uri")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| "ProxyAgent: options.uri is required".to_string())?;
        let token = value
            .get("token")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        (uri, token)
    } else {
        (trimmed.to_string(), None)
    };
    // undici requires an absolute proxy origin; catch the common mistake
    // ("localhost:8080") here rather than deep inside reqwest.
    if !uri.contains("://") {
        return Err(format!(
            "ProxyAgent: invalid proxy uri \"{uri}\" (expected an absolute URL like http://host:port)"
        ));
    }
    Ok(ProxyConfig { uri, token })
}

// The shared fetch proxy state — defined by perry-stdlib's fetch (bundled
// in the runtime archive) and by perry-ext-fetch; whichever copy is linked
// resolves this symbol. Passing a null `uri` clears the proxy.
unsafe extern "C" {
    fn js_fetch_set_global_proxy(
        uri_ptr: *const StringHeader,
        token_ptr: *const StringHeader,
    ) -> f64;
}

/// Apply a dispatcher's config to the native fetch client. Returns an
/// error string when the underlying client rejected the proxy config.
fn apply_dispatcher(dispatcher: &Dispatcher) -> Result<(), String> {
    match dispatcher {
        Dispatcher::Agent => {
            // SAFETY: null pointers are the documented "clear proxy" input.
            let ok = unsafe { js_fetch_set_global_proxy(std::ptr::null(), std::ptr::null()) };
            if ok == 1.0 {
                Ok(())
            } else {
                Err("setGlobalDispatcher: failed to clear the fetch proxy".to_string())
            }
        }
        Dispatcher::ProxyAgent(config) => {
            let uri = perry_ffi::alloc_string(&config.uri);
            let token = config
                .token
                .as_deref()
                .map(perry_ffi::alloc_string)
                .map(|s| s.as_raw() as *const StringHeader)
                .unwrap_or(std::ptr::null());
            // SAFETY: both pointers are freshly allocated Perry strings
            // (or null for "no token").
            let ok = unsafe { js_fetch_set_global_proxy(uri.as_raw(), token) };
            if ok == 1.0 {
                Ok(())
            } else {
                Err(format!(
                    "setGlobalDispatcher: native fetch rejected proxy uri \"{}\"",
                    config.uri
                ))
            }
        }
    }
}

/// `new ProxyAgent(uri | { uri, token? })`.
///
/// # Safety
/// `opts_ptr` must be null or a Perry-runtime `StringHeader` (the codegen's
/// `js_value_to_str_ptr_for_ffi` coercion: a URI string, or the options
/// object JSON-stringified).
#[no_mangle]
pub unsafe extern "C" fn js_undici_proxy_agent_new(opts_ptr: *const StringHeader) -> Handle {
    let raw = read_str(opts_ptr).unwrap_or_default();
    match parse_proxy_agent_options(&raw) {
        Ok(config) => register_handle(Dispatcher::ProxyAgent(config)),
        Err(msg) => throw_with_code(&msg, "UND_ERR_INVALID_ARG", ErrorKind::TypeError),
    }
}

/// `new Agent(options?)` — a direct-connection dispatcher. Options
/// (pool sizing, keep-alive tuning) are accepted and ignored: the native
/// reqwest client owns pooling.
///
/// # Safety
/// `_opts_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_undici_agent_new(_opts_ptr: *const StringHeader) -> Handle {
    register_handle(Dispatcher::Agent)
}

/// `setGlobalDispatcher(agent)` — apply the dispatcher's proxy config to
/// the native fetch client and remember it for `getGlobalDispatcher()`.
/// Returns undefined (NR_VOID row); failures throw.
#[no_mangle]
pub extern "C" fn js_undici_set_global_dispatcher(handle: Handle) {
    let applied = with_handle(handle, |dispatcher: &Dispatcher| {
        apply_dispatcher(dispatcher)
    });
    match applied {
        Some(Ok(())) => {
            *GLOBAL_DISPATCHER.lock().unwrap() = Some(handle);
        }
        Some(Err(msg)) => throw_with_code(&msg, "UND_ERR_INVALID_ARG", ErrorKind::TypeError),
        None => throw_with_code(
            "setGlobalDispatcher: argument must be an undici Agent or ProxyAgent",
            "UND_ERR_INVALID_ARG",
            ErrorKind::TypeError,
        ),
    }
}

/// `getGlobalDispatcher()` — the installed dispatcher, or a lazily created
/// default `Agent` (undici's implicit global agent) when none was set.
#[no_mangle]
pub extern "C" fn js_undici_get_global_dispatcher() -> Handle {
    let mut guard = GLOBAL_DISPATCHER.lock().unwrap();
    if let Some(handle) = *guard {
        return handle;
    }
    let handle = register_handle(Dispatcher::Agent);
    *guard = Some(handle);
    handle
}

/// `agent.close()` / `proxyAgent.close()` -> Promise<undefined>. The
/// native connection pool is process-global, so this only resolves.
#[no_mangle]
pub extern "C" fn js_undici_agent_close(_handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    promise.resolve_undefined();
    raw
}

/// `agent.destroy()` / `proxyAgent.destroy()` -> Promise<undefined>.
#[no_mangle]
pub extern "C" fn js_undici_agent_destroy(_handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    promise.resolve_undefined();
    raw
}

/// `request(url, options?)` — not implemented by perry-ext-undici. The
/// dispatcher/fetch subset is what perry serves natively; `request`'s
/// stream-based body mixin has no native counterpart yet.
///
/// # Safety
/// Both pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_undici_request(
    _url_ptr: *const StringHeader,
    _opts_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    promise.reject_string(
        "undici.request is not implemented by perry-ext-undici — use fetch() (undici's fetch is \
         served by perry's native fetch, and honors setGlobalDispatcher)",
    );
    raw
}

#[cfg(test)]
mod test_shims {
    //! Test-only definition of the `js_fetch_set_global_proxy` symbol the
    //! crate declares `extern` (provided by perry-stdlib / perry-ext-fetch
    //! in real links). Records calls so tests can assert the wiring.
    use perry_ffi::StringHeader;
    use std::sync::Mutex;

    pub(crate) static CALLS: Mutex<Vec<(Option<String>, Option<String>)>> = Mutex::new(Vec::new());

    #[no_mangle]
    pub unsafe extern "C" fn js_fetch_set_global_proxy(
        uri_ptr: *const StringHeader,
        token_ptr: *const StringHeader,
    ) -> f64 {
        let uri = super::read_str(uri_ptr);
        let token = super::read_str(token_ptr);
        // Mirror the real impl's contract: reject a clearly bogus URI so
        // the error path is testable.
        if uri.as_deref().is_some_and(|u| u.contains("invalid")) {
            return 0.0;
        }
        CALLS.lock().unwrap().push((uri, token));
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_uri_string() {
        let config = parse_proxy_agent_options("http://127.0.0.1:8080").unwrap();
        assert_eq!(config.uri, "http://127.0.0.1:8080");
        assert_eq!(config.token, None);
    }

    #[test]
    fn parses_options_object_with_token() {
        // What `js_value_to_str_ptr_for_ffi` hands us for
        // `new ProxyAgent({ uri: ..., token: ... })`.
        let config = parse_proxy_agent_options(
            r#"{"uri":"http://proxy.example:3128","token":"Basic dXNlcjpwYXNz"}"#,
        )
        .unwrap();
        assert_eq!(config.uri, "http://proxy.example:3128");
        assert_eq!(config.token.as_deref(), Some("Basic dXNlcjpwYXNz"));
    }

    #[test]
    fn options_object_without_token_is_fine() {
        let config = parse_proxy_agent_options(r#"{"uri":"https://proxy.example:443"}"#).unwrap();
        assert_eq!(config.uri, "https://proxy.example:443");
        assert_eq!(config.token, None);
    }

    #[test]
    fn extra_options_are_ignored() {
        // requestTls etc. are accepted-and-ignored, not an error.
        let config = parse_proxy_agent_options(
            r#"{"uri":"http://p:8080","requestTls":{"rejectUnauthorized":false}}"#,
        )
        .unwrap();
        assert_eq!(config.uri, "http://p:8080");
    }

    #[test]
    fn missing_uri_is_an_error() {
        let err = parse_proxy_agent_options(r#"{"token":"Basic x"}"#).unwrap_err();
        assert!(err.contains("uri is required"), "got: {err}");
    }

    #[test]
    fn relative_uri_is_an_error() {
        let err = parse_proxy_agent_options("localhost:8080").unwrap_err();
        assert!(err.contains("absolute URL"), "got: {err}");
    }

    #[test]
    fn empty_input_is_an_error() {
        assert!(parse_proxy_agent_options("").is_err());
        assert!(parse_proxy_agent_options("   ").is_err());
    }

    #[test]
    fn set_global_dispatcher_applies_proxy_and_tracks_handle() {
        let uri = perry_ffi::alloc_string(r#"{"uri":"http://127.0.0.1:9099","token":"Basic zz"}"#);
        let agent = unsafe { js_undici_proxy_agent_new(uri.as_raw()) };
        assert!(agent > 0);

        js_undici_set_global_dispatcher(agent);
        assert_eq!(js_undici_get_global_dispatcher(), agent);

        let calls = test_shims::CALLS.lock().unwrap();
        let last = calls.last().expect("proxy call recorded");
        assert_eq!(last.0.as_deref(), Some("http://127.0.0.1:9099"));
        assert_eq!(last.1.as_deref(), Some("Basic zz"));
    }

    #[test]
    fn installing_plain_agent_clears_proxy() {
        let agent = unsafe { js_undici_agent_new(std::ptr::null()) };
        js_undici_set_global_dispatcher(agent);
        let calls = test_shims::CALLS.lock().unwrap();
        let last = calls.last().expect("clear call recorded");
        assert_eq!(
            last.0, None,
            "Agent install must clear the proxy (null uri)"
        );
    }

    #[test]
    fn get_global_dispatcher_creates_default_agent() {
        // Runs in the same process as other tests; either a previous
        // install or the lazily created default must yield a live handle.
        let handle = js_undici_get_global_dispatcher();
        assert!(handle > 0);
        assert!(perry_ffi::handle_exists(handle));
    }
}
