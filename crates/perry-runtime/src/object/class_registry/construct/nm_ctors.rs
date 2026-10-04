//! Per-module constructor buckets (devirt phase 2).
//!
//! `new <namespace>.<Ctor>()` for node-module-namespaced constructors that the
//! old monolithic `js_new_function_construct` dispatched with a direct call to
//! the subsystem's `*_new` — statically pinning tty/fs/vm/tls/wasi/repl/stream/
//! readline handlers into every binary. Each is now a per-module fn reached only
//! through NM_CTOR_REGISTRY, registered by the same `js_nm_install_<module>()`
//! that codegen emits when the module is imported. `None` ⇒ not a ctor this
//! module owns; caller falls through (e.g. to the http/events/zlib dynamic
//! dispatchers, which already strip on their own).

use super::*;

/// Argument `n`, or `undefined` past the end.
#[inline]
unsafe fn nm_ctor_arg(args_ptr: *const f64, args_len: usize, n: usize) -> f64 {
    if !args_ptr.is_null() && args_len > n {
        *args_ptr.add(n)
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
}

pub(crate) unsafe fn nm_ctor_tty(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if matches!(method, "ReadStream" | "WriteStream") {
        let fd = nm_ctor_arg(args_ptr, args_len, 0);
        return Some(if method == "ReadStream" {
            crate::tty::js_tty_read_stream_new(fd)
        } else {
            crate::tty::js_tty_write_stream_new(fd)
        });
    }
    None
}

pub(crate) unsafe fn nm_ctor_child_process(
    _module: &str,
    method: &str,
    _args_ptr: *const f64,
    _args_len: usize,
) -> Option<f64> {
    (method == "ChildProcess").then(crate::child_process::cp_build_unstarted_child_process)
}

pub(crate) unsafe fn nm_ctor_cluster(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    (method == "Worker")
        .then(|| crate::cluster::js_cluster_worker_new(nm_ctor_arg(args_ptr, args_len, 0)))
}

pub(crate) unsafe fn nm_ctor_fs(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if method == "Utf8Stream" {
        return Some(crate::fs::js_fs_utf8_stream_new(nm_ctor_arg(
            args_ptr, args_len, 0,
        )));
    }
    if matches!(
        method,
        "ReadStream" | "FileReadStream" | "WriteStream" | "FileWriteStream"
    ) {
        let path = nm_ctor_arg(args_ptr, args_len, 0);
        let options = nm_ctor_arg(args_ptr, args_len, 1);
        return Some(if matches!(method, "ReadStream" | "FileReadStream") {
            crate::fs::js_fs_create_read_stream(path, options)
        } else {
            crate::fs::js_fs_create_write_stream(path, options)
        });
    }
    None
}

pub(crate) unsafe fn nm_ctor_vm(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if method == "Script" {
        let code = nm_ctor_arg(args_ptr, args_len, 0);
        let options = nm_ctor_arg(args_ptr, args_len, 1);
        return Some(super::super::brand_vm_script_instance(
            crate::node_vm::js_vm_script_new(code, options),
        ));
    }
    None
}

pub(crate) unsafe fn nm_ctor_tls(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if method == "SecureContext" {
        return Some(crate::tls::js_tls_secure_context_new(nm_ctor_arg(
            args_ptr, args_len, 0,
        )));
    }
    crate::tls::construct_registered_tls_class(method, args_ptr, args_len)
}

pub(crate) unsafe fn nm_ctor_wasi(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if method == "WASI" {
        return Some(crate::wasi::js_wasi_new(nm_ctor_arg(args_ptr, args_len, 0)));
    }
    None
}

pub(crate) unsafe fn nm_ctor_readline(
    module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if module == "readline/promises" && method == "Readline" {
        let output = nm_ctor_arg(args_ptr, args_len, 0);
        let options = nm_ctor_arg(args_ptr, args_len, 1);
        return Some(crate::node_submodules::js_readline_promises_readline_new(
            output, options,
        ));
    }
    None
}

pub(crate) unsafe fn nm_ctor_repl(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if matches!(method, "Recoverable" | "REPLServer") {
        let first = nm_ctor_arg(args_ptr, args_len, 0);
        return Some(if method == "Recoverable" {
            crate::node_repl::js_repl_recoverable_new(first)
        } else {
            crate::node_repl::js_repl_repl_server_new(first)
        });
    }
    None
}

/// #4995: `new EE()` where `EE = require('events')` or came in as a default /
/// namespace import (`import EE from 'events'`, `import * as ev from
/// 'events'; new ev.EventEmitter()`). The callee is the bound
/// `events.EventEmitter` export value; the instance is the same ordinary
/// object the static `new EventEmitter()` builds (#10508).
pub(crate) unsafe fn nm_ctor_events(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let options = nm_ctor_arg(args_ptr, args_len, 0);
    match method {
        "EventEmitter" => Some(crate::node_stream::js_event_emitter_object_new(options)),
        "EventEmitterAsyncResource" => {
            Some(crate::node_stream::js_event_emitter_async_resource_object_new(options))
        }
        _ => None,
    }
}

pub(crate) unsafe fn nm_ctor_stream(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if matches!(
        method,
        "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough"
    ) {
        let opts = nm_ctor_arg(args_ptr, args_len, 0);
        return Some(match method {
            "Readable" => crate::node_stream::js_node_stream_readable_new(opts),
            "Writable" => crate::node_stream::js_node_stream_writable_new(opts),
            "Duplex" => crate::node_stream::js_node_stream_duplex_new(opts),
            "Transform" => crate::node_stream::js_node_stream_transform_new(opts),
            "PassThrough" => crate::node_stream::js_node_stream_passthrough_new(opts),
            _ => unreachable!(),
        });
    }
    // #10430: `new Stream()` (legacy `Stream`, i.e. `new (require('stream'))()`)
    // is an ordinary instance of `Stream.prototype`, whose EventEmitter methods
    // act on the receiver. Build it the way an ordinary function constructor's
    // instance is built (the constructor's stable synthetic class id plus a
    // class-default link to its `prototype`), not via `Object.create`, which
    // mints a fresh synthetic class per call. Without this arm the instance
    // had no `on`/`emit` and was not `instanceof Stream`.
    if method == "Stream" {
        let scope = crate::gc::RuntimeHandleScope::new();
        let ctor = scope.root_nanbox_f64(crate::object::bound_native_callable_export_value(
            "stream", "Stream",
        ));
        let cid = synthetic_class_id_for_function(ctor.get_nanbox_f64());
        let instance = scope.root_raw_mut_ptr(js_object_alloc(cid, 0));
        let proto = crate::closure::closure_get_dynamic_prop(
            (ctor.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
            "prototype",
        );
        if crate::value::JSValue::from_bits(proto.to_bits()).is_pointer() {
            instance.with_mut_ptr::<ObjectHeader, _>(|obj| {
                super::super::super::prototype_chain::object_link_class_default_prototype(
                    obj as usize,
                    proto.to_bits(),
                )
            });
        }
        return Some(
            instance
                .with_mut_ptr::<ObjectHeader, _>(|obj| crate::value::js_nanbox_pointer(obj as i64)),
        );
    }
    None
}
