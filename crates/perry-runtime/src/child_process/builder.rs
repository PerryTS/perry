use super::*;

use crate::closure::{js_closure_alloc, js_closure_set_capture_ptr};
use crate::object::{js_object_alloc_with_shape, js_object_set_field, ObjectHeader};
use crate::value::JSValue;

// Shape-id band kept clear of node_stream (0x7FFF_FE60+), fs streams
// (0x7FFF_FE40), and weakref (0x7FFF_FE10+).
pub(crate) const CP_SHAPE_ID: u32 = 0x7FFF_FD00;
// The pre-socket stdio objects (unix pipe slots are net sockets).
#[cfg(not(unix))]
pub(crate) const CP_READABLE_SHAPE_ID: u32 = 0x7FFF_FD40;
#[cfg(not(unix))]
pub(crate) const CP_WRITABLE_SHAPE_ID: u32 = 0x7FFF_FD80;

// ----- object construction -----

/// A child-process method body's static info: what a method table lists and
/// `cp_build_object` allocates from (`crate::fn_info!(body, N)`).
pub(crate) type CpFn = *const crate::closure::JsFunctionInfo;

/// Allocate a heap object whose method-name fields each hold a closure capturing
/// the object itself in slot 0 (so method bodies recover `this`).
pub(crate) fn cp_build_object(methods: &[(&str, CpFn)], shape_id: u32) -> *mut ObjectHeader {
    let mut packed: Vec<u8> = Vec::new();
    for (name, _) in methods {
        packed.extend_from_slice(name.as_bytes());
        packed.push(0);
    }
    let obj = js_object_alloc_with_shape(
        shape_id,
        methods.len() as u32,
        packed.as_ptr(),
        packed.len() as u32,
    );
    let this_bits = JSValue::pointer(obj as *const u8).bits();
    for (i, (_name, func)) in methods.iter().enumerate() {
        let closure = js_closure_alloc(*func, 1);
        js_closure_set_capture_ptr(closure, 0, this_bits as i64);
        js_object_set_field(obj, i as u32, JSValue::pointer(closure as *const u8));
    }
    obj
}

pub(crate) fn cp_install_dispose(cp: f64) {
    let Some(obj) = cp_object_ptr(cp) else {
        return;
    };

    let closure = js_closure_alloc(
        crate::fn_info!(cp_method_dispose, 0; with_declared(0), with_length(0)),
        1,
    );
    if closure.is_null() {
        return;
    }
    js_closure_set_capture_ptr(closure, 0, cp.to_bits() as i64);
    crate::object::set_bound_native_closure_name(closure, "");
    crate::object::set_builtin_closure_length(closure as usize, 0);
    let dispose_value = cp_box_ptr(closure as *const u8);

    let hidden_attrs = crate::object::PropertyAttrs::new(true, false, true);
    for key in ["__perry_dispose__", "@@__perry_wk_dispose"] {
        cp_set_field(cp, key.as_bytes(), dispose_value);
        crate::object::set_builtin_property_attrs(obj as usize, key.to_string(), hidden_attrs);
    }

    let dispose_sym = crate::symbol::well_known_symbol("dispose");
    if !dispose_sym.is_null() {
        let dispose_sym_value = cp_box_ptr(dispose_sym as *const u8);
        unsafe {
            crate::symbol::js_object_set_symbol_property(cp, dispose_sym_value, dispose_value);
        }
    }
}

/// A pipe slot's parent end as a `net.Socket` that owns it, as Node's
/// `createSocket(pipe, readable)`: `readable` is false for stdin (slot 0).
/// The descriptor is consumed. Throws when the program links no net provider,
/// since the pipe has no other representation.
#[cfg(unix)]
pub(crate) fn cp_pipe_socket(readable: bool, pipe: std::os::fd::OwnedFd) -> f64 {
    use std::os::fd::IntoRawFd;
    let scope = crate::gc::RuntimeHandleScope::new();
    let options =
        scope.root_nanbox_f64(cp_box_ptr(crate::object::js_object_alloc(0, 1) as *const u8));
    cp_set_field(
        options.get_nanbox_f64(),
        b"readable",
        if readable {
            TAG_TRUE_F64
        } else {
            TAG_FALSE_F64
        },
    );
    let fd = pipe.into_raw_fd();
    let args = [options.get_nanbox_f64(), fd as f64];
    cp_install_net_provider();
    match unsafe { crate::object::net_provider_call("Socket.adoptPipe", &args) } {
        Some(socket) => socket,
        None => {
            // SAFETY: the provider never saw `fd`; it is still ours to close.
            unsafe { libc::close(fd) };
            crate::fs::validate::throw_type_error_with_code(
                "child_process stdio pipes need node:net, which this program does not link",
                "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM",
            )
        }
    }
}

// ELF: the lazy hook reference below is a hidden undefined weak symbol, so it
// resolves at link time (to the hook or to null) and adds no dynamic
// relocation to every startup.
#[cfg(all(
    unix,
    not(target_vendor = "apple"),
    not(any(test, feature = "test-layout-descriptors"))
))]
core::arch::global_asm!(".weak js_ext_net_nm_install\n.hidden js_ext_net_nm_install");

/// Register node:net's provider at the first pipe socket. A `child_process`
/// program links perry-ext-net without running its install hook at startup
/// (the driver leaves it out of the entry prologue), so the hook runs here,
/// once: the dispatcher it registers is the "installed" state. The reference
/// is weak: a program without perry-ext-net resolves it to null and
/// [`cp_pipe_socket`] reports the missing provider.
#[cfg(unix)]
fn cp_install_net_provider() {
    if !crate::value::JS_NATIVE_NET_DISPATCH
        .load(std::sync::atomic::Ordering::SeqCst)
        .is_null()
    {
        return;
    }
    // Test harnesses link no program driver (Darwin's `-U` admission comes
    // from it, `link/wrapper_hooks.rs`); a crate test registers the provider
    // itself, as with the runtime layouts.
    #[cfg(not(any(test, feature = "test-layout-descriptors")))]
    let hook = {
        extern "C" {
            #[linkage = "extern_weak"]
            static js_ext_net_nm_install: *const u8;
        }
        // SAFETY: an extern_weak static reads as the symbol's address, or null.
        unsafe { js_ext_net_nm_install }
    };
    #[cfg(any(test, feature = "test-layout-descriptors"))]
    let hook: *const u8 = std::ptr::null();
    if !hook.is_null() {
        // SAFETY: the symbol is perry-ext-net's `extern "C" fn()` install hook.
        let hook: unsafe extern "C" fn() = unsafe { std::mem::transmute(hook) };
        unsafe { hook() };
    }
}

/// Build a stdout/stderr Readable-shaped EventEmitter.
#[cfg(not(unix))]
pub(crate) fn cp_build_readable() -> f64 {
    let methods: [(&str, CpFn); 13] = [
        ("on", crate::fn_info!(cp_method_on, 2; with_declared(2))),
        ("once", crate::fn_info!(cp_method_on, 2; with_declared(2))),
        (
            "addListener",
            crate::fn_info!(cp_method_on, 2; with_declared(2)),
        ),
        (
            "prependListener",
            crate::fn_info!(cp_method_on, 2; with_declared(2)),
        ),
        (
            "off",
            crate::fn_info!(cp_method_remove_listener, 2; with_declared(2)),
        ),
        (
            "removeListener",
            crate::fn_info!(cp_method_remove_listener, 2; with_declared(2)),
        ),
        ("emit", crate::fn_info!(cp_method_emit, 2; with_declared(2))),
        (
            "pause",
            crate::fn_info!(cp_method_this0, 0; with_declared(0)),
        ),
        (
            "resume",
            crate::fn_info!(cp_method_this0, 0; with_declared(0)),
        ),
        (
            "destroy",
            crate::fn_info!(cp_method_this0, 0; with_declared(0)),
        ),
        (
            "setEncoding",
            crate::fn_info!(cp_method_set_encoding, 1; with_declared(1)),
        ),
        ("read", crate::fn_info!(cp_method_read, 1; with_declared(1))),
        ("pipe", crate::fn_info!(cp_method_pipe, 1; with_declared(1))),
    ];
    let obj = cp_build_object(&methods, CP_READABLE_SHAPE_ID + methods.len() as u32);
    let val = cp_box_ptr(obj as *const u8);
    cp_set_field(val, b"readable", TAG_TRUE_F64);
    cp_set_field(val, b"readableEnded", TAG_FALSE_F64);
    cp_set_field(val, b"destroyed", TAG_FALSE_F64);
    // A child's `stdout`/`stderr` must be async-iterable, like Node's: both
    // `for await (const chunk of child.stdout)` and the `isAsyncIterable` probe
    // every stream-consuming library runs —
    // `typeof stream[Symbol.asyncIterator] === "function"` — depend on it.
    // Without the symbol, `get-stream` (used by execa) rejects the stream with
    // "The first argument must be a Readable, a ReadableStream, or an async
    // iterable", which silently aborted the background downloads of a large
    // esbuild-bundled CLI app.
    //
    // Reuse node:stream's iterator: it is event-driven (it attaches persistent
    // `data`/`end`/`error` listeners and settles a promise per pull), so it
    // drives this emitter-backed object as-is — `cp_emit` forwards to node:stream's
    // listener registry so those listeners fire.
    crate::node_stream::async_iterator::install_foreign_readable_async_iterator_symbol(val);
    val
}

/// Build a stdin Writable-shaped EventEmitter.
#[cfg(not(unix))]
pub(crate) fn cp_build_writable() -> f64 {
    let methods: [(&str, CpFn); 11] = [
        ("on", crate::fn_info!(cp_method_on, 2; with_declared(2))),
        ("once", crate::fn_info!(cp_method_on, 2; with_declared(2))),
        (
            "addListener",
            crate::fn_info!(cp_method_on, 2; with_declared(2)),
        ),
        (
            "removeListener",
            crate::fn_info!(cp_method_remove_listener, 2; with_declared(2)),
        ),
        (
            "off",
            crate::fn_info!(cp_method_remove_listener, 2; with_declared(2)),
        ),
        ("emit", crate::fn_info!(cp_method_emit, 2; with_declared(2))),
        (
            "write",
            crate::fn_info!(cp_method_stdin_write, 3; with_declared(3)),
        ),
        (
            "end",
            crate::fn_info!(cp_method_stdin_end, 3; with_declared(3)),
        ),
        (
            "destroy",
            crate::fn_info!(cp_method_this0, 0; with_declared(0)),
        ),
        (
            "cork",
            crate::fn_info!(cp_method_this0, 0; with_declared(0)),
        ),
        (
            "uncork",
            crate::fn_info!(cp_method_this0, 0; with_declared(0)),
        ),
    ];
    let obj = cp_build_object(&methods, CP_WRITABLE_SHAPE_ID + methods.len() as u32);
    let val = cp_box_ptr(obj as *const u8);
    cp_set_field(val, b"readable", TAG_FALSE_F64);
    cp_set_field(val, b"writable", TAG_TRUE_F64);
    cp_set_field(val, b"destroyed", TAG_FALSE_F64);
    // #9493: the back-pressure observables `stdin.write()` maintains.
    cp_set_field(val, b"writableLength", 0.0);
    cp_set_field(
        val,
        b"writableHighWaterMark",
        reactor::CP_STDIN_HIGH_WATER_MARK as f64,
    );
    cp_set_field(val, b"writableNeedDrain", TAG_FALSE_F64);
    val
}

/// Build the inert public `new ChildProcess()` instance. Normal `spawn()` and
/// `fork()` construct their live variants in the reactor; this low-level Node
/// API only needs the initial observable shape plus its validating `.spawn`.
pub(crate) fn cp_build_unstarted_child_process() -> f64 {
    let methods: [(&str, CpFn); 11] = [
        ("on", crate::fn_info!(cp_method_on, 2; with_declared(2))),
        ("once", crate::fn_info!(cp_method_on, 2; with_declared(2))),
        (
            "addListener",
            crate::fn_info!(cp_method_on, 2; with_declared(2)),
        ),
        (
            "removeListener",
            crate::fn_info!(cp_method_remove_listener, 2; with_declared(2)),
        ),
        (
            "off",
            crate::fn_info!(cp_method_remove_listener, 2; with_declared(2)),
        ),
        ("emit", crate::fn_info!(cp_method_emit, 2; with_declared(2))),
        (
            "removeAllListeners",
            crate::fn_info!(cp_method_remove_all_listeners, 1; with_declared(1)),
        ),
        ("kill", crate::fn_info!(cp_method_kill, 1; with_declared(1))),
        ("ref", crate::fn_info!(cp_method_ref, 0; with_declared(0))),
        (
            "unref",
            crate::fn_info!(cp_method_unref, 0; with_declared(0)),
        ),
        (
            "spawn",
            crate::fn_info!(cp_method_child_spawn, 1; with_declared(1)),
        ),
    ];
    let obj = cp_build_object(&methods, CP_SHAPE_ID + 0x60 + methods.len() as u32);
    let scope = crate::gc::RuntimeHandleScope::new();
    let child = scope.root_nanbox_f64(cp_box_ptr(obj as *const u8));
    cp_set_field(child.get_nanbox_f64(), b"connected", TAG_FALSE_F64);
    cp_set_field(child.get_nanbox_f64(), b"killed", TAG_FALSE_F64);
    cp_set_field(child.get_nanbox_f64(), b"exitCode", TAG_NULL_F64);
    cp_set_field(child.get_nanbox_f64(), b"signalCode", TAG_NULL_F64);
    cp_set_field(child.get_nanbox_f64(), b"spawnfile", TAG_NULL_F64);

    let constructor = crate::object::bound_native_callable_export_value(
        &crate::object::native_module::namespaces::child_process::NM_CHILD_PROCESS,
        "child_process",
        "ChildProcess",
    );
    let constructor =
        unsafe { crate::object::callable_exports::ensure_child_process_prototype(constructor) };
    let raw = (constructor.to_bits() & crate::value::POINTER_MASK) as usize;
    let prototype = crate::closure::closure_get_dynamic_prop(raw, "prototype");
    if let Some(obj) = cp_object_ptr(child.get_nanbox_f64()) {
        crate::object::prototype_chain::object_set_static_prototype(
            obj as usize,
            prototype.to_bits(),
        );
    }
    child.get_nanbox_f64()
}

/// Public constructor hook for the codegen `new ChildProcess()` fast path.
#[no_mangle]
pub extern "C" fn js_child_process_new() -> f64 {
    cp_build_unstarted_child_process()
}
