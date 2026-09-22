//! `node:http` / `node:https` / `node:http2` exports reached as VALUES
//! (#2533, #4904, #10428).
//!
//! The method-call form on an import binding (`http.createServer(...)`) lowers
//! through codegen's static native table. A captured / aliased export — a
//! CommonJS `require('http').createServer(cb)`, `const { request } =
//! require('http')`, ws' `const request = isSecure ? https.request :
//! http.request`, fastify's `http.createServer(options.http, handler)` — instead
//! reaches perry-runtime's http dispatch bucket as a bound-method closure, and
//! the runtime calls back through `js_set_native_http_dispatch`.
//!
//! This dispatcher used to live in perry-stdlib and was registered only when
//! the stdlib was rebuilt with `external-http-server-pump`. The prebuilt
//! archives `PERRY_NO_AUTO_OPTIMIZE=1` links never have that feature, so every
//! value-form call returned `undefined`. It now lives with the implementations
//! it routes to and is registered by `js_ext_http_nm_install`, the install
//! symbol codegen emits wherever it materializes an http/https/http2 namespace
//! or bound export — in every compile mode.

use perry_ffi::{ArrayHeader, JsValue, TransientRootScope};

use super::types::js_value_is_closure;

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

type HttpDispatchFn =
    unsafe extern "C" fn(*const u8, usize, *const u8, usize, *const f64, usize) -> f64;

extern "C" {
    fn js_set_native_http_dispatch(f: HttpDispatchFn);
    fn js_nm_install_http();
}

/// Install the runtime's http dispatch bucket together with this crate's
/// export dispatcher, so the first value-form call already has a provider.
#[no_mangle]
pub unsafe extern "C" fn js_ext_http_nm_install() {
    js_set_native_http_dispatch(js_ext_http_native_dispatch);
    js_nm_install_http();
}

/// The caller's argument buffer. Slots are re-read at each use rather than
/// borrowed as a slice: the factories below can collect, and a moving
/// collection rewrites rooted slots in place.
#[derive(Clone, Copy)]
struct Args {
    ptr: *const f64,
    len: usize,
}

impl Args {
    unsafe fn get(self, n: usize) -> f64 {
        if n < self.len {
            *self.ptr.add(n)
        } else {
            f64::from_bits(TAG_UNDEFINED)
        }
    }
}

fn handle_value(handle: i64) -> f64 {
    if handle == 0 {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        f64::from_bits(POINTER_TAG | (handle as u64 & POINTER_MASK))
    }
}

/// `get` / `request` keep the COMPLETE argument list and go through the same
/// overload normalizer as a statically-known call (#4975): picking only the
/// first non-closure argument lost `(url, options, callback)` and treated
/// WHATWG URL objects as plain option bags.
unsafe fn client_overload(module: &str, method: &str, args: Args) -> i64 {
    let roots = TransientRootScope::enter();
    let rooted_args = (0..args.len)
        .map(|n| roots.root_nanbox(args.get(n)))
        .collect::<Vec<_>>();
    let array = roots.root_addr(perry_ffi::js_array_alloc(args.len as u32) as i64);
    for arg in rooted_args {
        let _ = perry_ffi::js_array_push(
            array.get() as *mut ArrayHeader,
            JsValue::from_bits(arg.get().to_bits()),
        );
    }
    let array = array.get();
    match (module, method) {
        ("http", "get") => crate::js_http_get_overload(array),
        ("http", "request") => crate::js_http_request_overload(array),
        ("https", "get") => crate::js_https_get_overload(array),
        _ => crate::js_https_request_overload(array),
    }
}

/// Node's overloads are `createServer([options][, requestListener])`, while
/// `@hono/node-server` calls `createServer(serverOptions, requestListener)`.
/// Each arg is classified by type rather than position — the function/closure
/// arg is the handler, the remaining object arg is the options — so both
/// orders work.
unsafe fn create_server(module: &str, method: &str, args: Args) -> i64 {
    let mut handler_ptr: i64 = 0;
    let mut options = f64::from_bits(TAG_UNDEFINED);
    for n in 0..args.len.min(2) {
        let arg = args.get(n);
        if js_value_is_closure(arg.to_bits() as i64) != 0 {
            handler_ptr = (arg.to_bits() & POINTER_MASK) as i64;
        } else if JsValue::from_bits(arg.to_bits()).is_pointer() {
            options = arg;
        }
    }
    let handler = handle_value(handler_ptr);
    match module {
        "http" => super::js_node_http_create_server_with_options(options, handler),
        "https" => super::js_node_https_create_server(options, handler_ptr),
        "http2" if method == "createSecureServer" => {
            super::js_node_http2_create_secure_server(options, handler_ptr)
        }
        "http2" => super::js_node_http2_create_server(options, handler),
        _ => 0,
    }
}

/// Runtime callback for the http/https/http2 (and `bun.serve`) value forms.
/// Each arm calls the entry point the static native-table row for that export
/// uses; construction (`new http.Agent(opts)`, `new http.IncomingMessage(s)`)
/// reaches the same arms through perry-runtime's class-registry http arm.
///
/// # Safety
/// The name pointers must describe UTF-8 bytes; `args_ptr` must point to
/// `args_len` NaN-boxed values (or be null when `args_len` is 0).
#[no_mangle]
pub unsafe extern "C" fn js_ext_http_native_dispatch(
    module_ptr: *const u8,
    module_len: usize,
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let name = |ptr: *const u8, len: usize| {
        if ptr.is_null() {
            ""
        } else {
            std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap_or("")
        }
    };
    let module = name(module_ptr, module_len);
    let method = name(method_ptr, method_len);
    let args = Args {
        ptr: args_ptr,
        len: if args_ptr.is_null() { 0 } else { args_len },
    };
    let arg = |n: usize| args.get(n);
    let handle = match (module, method) {
        ("bun", "serve") => super::bun_server::js_bun_serve(arg(0)),
        ("http", "OutgoingMessage") => super::js_node_http_outgoing_message_new(),
        ("http", "IncomingMessage") => super::js_node_http_incoming_message_standalone_new(arg(0)),
        ("http", "ServerResponse") => super::js_node_http_server_response_standalone_new(arg(0)),
        ("http" | "https", "get" | "request") => client_overload(module, method, args),
        ("http", "Agent") => crate::js_http_agent_new(arg(0)),
        ("https", "Agent") => crate::js_https_agent_new(arg(0)),
        ("http", "ClientRequest") => crate::js_http_client_request_standalone_new(arg(0)),
        ("http" | "https" | "http2", _) => create_server(module, method, args),
        _ => 0,
    };
    handle_value(handle)
}
