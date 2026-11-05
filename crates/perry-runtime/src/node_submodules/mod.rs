//! Issue #841 — wire up named exports + namespace imports for five
//! Node.js submodules that Perry's manifest had registered but whose
//! FFI export tables defaulted to a `TAG_TRUE` sentinel cell:
//!
//!   - `node:timers/promises` (setTimeout / setImmediate / setInterval / scheduler.*)
//!   - `node:readline/promises` (createInterface, Interface, Readline)
//!   - `node:stream/promises` (pipeline, finished)
//!   - `node:stream/consumers` (text, json, buffer, arrayBuffer, bytes, blob)
//!   - `node:sys` (deprecated alias for node:util — re-exports format, inspect, etc.)
//!
//! Pre-fix `import { setTimeout } from "node:timers/promises"; typeof setTimeout`
//! reported `"boolean"` (the value was literally `true`) and `import * as ns
//! from "node:..."` errored at compile time with the "switch to named imports"
//! diagnostic. This module ships per-export function singletons whose `typeof`
//! is `"function"`, plus per-submodule namespace stubs whose properties point
//! at the same singletons.
//!
//! Most thunks are deliberately minimal — they throw `Error("<api> is not yet
//! implemented in Perry")` when invoked. `node:stream/consumers` is the first
//! submodule here with concrete behavior, so consuming code can import and use
//! its helpers while the broader #793 Node compatibility roadmap continues.

use std::cell::RefCell;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::closure::{
    js_closure_alloc, js_closure_call0, js_closure_call1, js_closure_call2,
    js_closure_get_capture_ptr, js_closure_set_capture_ptr, ClosureHeader, JsFunctionInfo,
};
use crate::feature_hooks::Hook;
use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_set_field_by_name, ObjectHeader,
    PropertyAttrs,
};
use crate::string::js_string_from_bytes;
use crate::value::JSValue;

pub(crate) mod diagnostics;
pub(crate) mod diagnostics_gc;
pub use diagnostics::*;
pub(crate) mod diagnostics_tail;
pub(crate) use diagnostics_tail::*;

/// One entry per named export of one submodule: its name and its thunk's
/// info (the declared arity, or the rest parameter of a variadic export).
struct ExportSpec {
    name: &'static str,
    info: &'static JsFunctionInfo,
}

/// One entry per submodule. `exports` lists every named export the
/// codegen / parity tests reach for; the codegen's lookup is keyed by
/// `(submodule_key, export_name)` and falls back to `TAG_TRUE` if no
/// matching entry is found (preserving the pre-#841 behavior for any
/// future export Perry doesn't yet know about).
pub(super) struct SubmoduleSpec {
    /// Stable key — matches the prefix used in the generated FFI symbol
    /// names (`js_node_submod_<key>_export_<name>`).
    key: &'static str,
    /// The submodule's ordinal: its registry slot and its singleton slots.
    bucket: SubmodBucket,
    exports: &'static [&'static ExportSpec],
}

// ----- thunks -----
//
// One thunk per (submodule, export). All thunks share the same shape:
// they raise an explicit `Error` describing what's missing. Closure
// dispatch invokes them via `js_closure_call0` / `js_closure_call1`
// regardless of declared arity, so a single `(_closure, _arg) -> f64`
// signature is sufficient — Perry's closure ABI tolerates an arg shape
// mismatch on the receiving side (the value is just ignored).

macro_rules! thunk {
    ($name:ident, $msg:expr) => {
        #[allow(non_snake_case)] // thunk name mirrors JS API surface
        pub(crate) extern "C" fn $name(
            _closure: *const crate::closure::ClosureHeader,
            _this: crate::closure::JsThis,
            _arg: f64,
        ) -> f64 {
            let msg: &'static str = $msg;
            let bytes = msg.as_bytes();
            let header = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            let err = crate::error::js_error_new_with_message(header);
            let bits = crate::value::JSValue::pointer(err as *const u8).bits();
            crate::exception::js_throw(f64::from_bits(bits))
        }
    };
}

pub(crate) mod blob;
pub(crate) mod consumers;
mod fs_promises;
mod hono_jsx;
mod registry_backed;
mod stream_promises;
pub(crate) mod test;
mod timers;
mod trace_events;
mod zlib;
pub use zlib::{js_zlib_resolve_level, js_zlib_validate_buffer_arg, js_zlib_validate_options};

// #1671: hono/jsx/server + hono/jsx/streaming. Re-export the stream-creation
// registration so perry-stdlib's `bundled-streams` init can wire it up.
pub use hono_jsx::js_register_jsx_render_stream;
use hono_jsx::{
    thunk_hono_fragment, thunk_hono_jsx, thunk_hono_jsxnode, thunk_hono_jsxs,
    thunk_hono_render_to_readable_stream, thunk_hono_suspense,
};

pub(crate) use consumers::{consume_array_buffer, consume_bytes, consume_json, consume_text};
// Re-export at the `node_submodules` root so perry-stdlib's
// `perry_runtime::node_submodules::js_register_stream_consumer_callbacks`
// call site keeps resolving after the consumers split.
pub use consumers::js_register_stream_consumer_callbacks;
pub(crate) use fs_promises::js_readline_promises_readline_new;
use fs_promises::thunk_fs_promises_constants;
use registry_backed::native_namespace_as_default;
pub(crate) use registry_backed::{
    registry_backed_module_callable, registry_backed_module_export, stream_promises_export_callable,
};
use stream_promises::value_from_ptr;
#[cfg(test)]
use stream_promises::{thunk_streamP_finished, thunk_streamP_pipeline};
use timers::{
    timers_promises_scheduler, timers_promises_scheduler_wait, timers_promises_scheduler_yield,
};
pub(crate) use trace_events::{flush_trace_events_output, init_trace_events_runtime};

// node:sys is a deprecated alias for node:util. Known util-backed
// exports are rebound to util's callable singletons below so identity
// checks like `sys.format === util.format` match Node; these thunks
// remain as fallbacks for sys names Perry does not yet expose through
// the util native module table.
thunk!(thunk_sys_format, "node:sys.format is not yet implemented in Perry (use node:util.format; node:sys is deprecated).");
thunk!(thunk_sys_inspect, "node:sys.inspect is not yet implemented in Perry (use node:util.inspect; node:sys is deprecated).");
thunk!(thunk_sys_debuglog, "node:sys.debuglog is not yet implemented in Perry (use node:util.debuglog; node:sys is deprecated).");
thunk!(thunk_sys_deprecate, "node:sys.deprecate is not yet implemented in Perry (use node:util.deprecate; node:sys is deprecated).");
thunk!(thunk_sys_promisify, "node:sys.promisify is not yet implemented in Perry (use node:util.promisify; node:sys is deprecated).");
thunk!(thunk_sys_callbackify, "node:sys.callbackify is not yet implemented in Perry (use node:util.callbackify; node:sys is deprecated).");
thunk!(thunk_sys_isArray, "node:sys.isArray is not yet implemented in Perry (use node:util.isArray; node:sys is deprecated).");

// #1545: node:stream/web (WHATWG Web Streams). The named-export bindings
// exist so `typeof ReadableStream === "function"` and `import * as web from
// "node:stream/web"` work. Construction (`new ReadableStream(...)`,
// `new CountQueuingStrategy(...)`, …) is handled in codegen's
// builtin-constructor dispatch (lower_call/builtin.rs), which routes by the
// textual class name regardless of how it was imported — so this thunk only
// runs when a Web Streams class is *called* without `new`, which throws a
// TypeError in Node too. One shared thunk covers all 17 exported classes.
thunk!(
    thunk_stream_web_ctor,
    "Web Streams constructors (node:stream/web) require the 'new' operator."
);

extern "C" fn thunk_vm_create_context(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    sandbox: f64,
    options: f64,
) -> f64 {
    crate::object::js_vm_create_context(sandbox, options)
}

// ----- submodule table -----

// ----- per-submodule specs (devirt) -----
// Imports of bodies whose owning definition sits in plain source are spelled
// out here, in plain source too: the link name is built from the written
// tokens, which the shared export list cannot reproduce through its macros.
static IMPORT_THUNK_TEST: &JsFunctionInfo = crate::fn_info!(reference import crate::node_submodules::test::runner::thunk_test, 3; with_declared(3));
static IMPORT_THUNK_TEST_ONLY: &JsFunctionInfo = crate::fn_info!(reference import crate::node_submodules::test::runner::thunk_test_only, 3; with_declared(3));
static IMPORT_THUNK_TEST_SKIP: &JsFunctionInfo = crate::fn_info!(reference import crate::node_submodules::test::runner::thunk_test_skip, 3; with_declared(3));
static IMPORT_THUNK_TEST_TODO: &JsFunctionInfo = crate::fn_info!(reference import crate::node_submodules::test::runner::thunk_test_todo, 3; with_declared(3));

// Shared with codegen through perry-dispatch; a module here so the realm
// manifest reads its declarations as this crate's.
#[allow(dead_code)]
mod native_submodule_exports;
mod static_export_declarations;
use static_export_declarations::*;

// ----- submodule registry (devirt) -----
use std::sync::atomic::AtomicPtr;
#[derive(Copy, Clone)]
#[repr(usize)]
enum SubmodBucket {
    Vm,
    Timers,
    TimersPromises,
    FsPromises,
    ReadlinePromises,
    StreamPromises,
    StreamConsumers,
    StreamWeb,
    HonoJsxServer,
    HonoJsxStreaming,
    Sys,
    DiagnosticsChannel,
    TraceEvents,
    Test,
    TestReporters,
}
const SUBMOD_COUNT: usize = 15;
static SUBMOD_REGISTRY: [AtomicPtr<SubmoduleSpec>; SUBMOD_COUNT] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; SUBMOD_COUNT];
fn submod_index(key: &str) -> Option<SubmodBucket> {
    match key {
        "vm" => Some(SubmodBucket::Vm),
        "timers" => Some(SubmodBucket::Timers),
        "timers_promises" => Some(SubmodBucket::TimersPromises),
        "fs_promises" => Some(SubmodBucket::FsPromises),
        "readline_promises" => Some(SubmodBucket::ReadlinePromises),
        "stream_promises" => Some(SubmodBucket::StreamPromises),
        "stream_consumers" => Some(SubmodBucket::StreamConsumers),
        "stream_web" => Some(SubmodBucket::StreamWeb),
        "hono_jsx_server" => Some(SubmodBucket::HonoJsxServer),
        "hono_jsx_streaming" => Some(SubmodBucket::HonoJsxStreaming),
        "sys" => Some(SubmodBucket::Sys),
        "diagnostics_channel" => Some(SubmodBucket::DiagnosticsChannel),
        "trace_events" => Some(SubmodBucket::TraceEvents),
        "test" => Some(SubmodBucket::Test),
        "test_reporters" => Some(SubmodBucket::TestReporters),
        _ => None,
    }
}
fn find_submodule(key: &str) -> Option<&'static SubmoduleSpec> {
    let b = submod_index(key)?;
    let p = SUBMOD_REGISTRY[b as usize].load(Ordering::Relaxed);
    if !p.is_null() {
        return Some(unsafe { &*(p as *const SubmoduleSpec) });
    }
    // Unit tests call the namespace/export helpers directly, without the
    // codegen-emitted `js_node_submod_install_<key>()` that precedes use in real
    // programs. Lazily populate the registry so tests exercise the real lookup
    // path. (Not compiled into production builds.)
    #[cfg(test)]
    {
        js_node_submod_install_all();
        let p = SUBMOD_REGISTRY[b as usize].load(Ordering::Relaxed);
        if !p.is_null() {
            return Some(unsafe { &*(p as *const SubmoduleSpec) });
        }
    }
    None
}

/// Test-only: whether `key` names a registered submodule spec. #6651 —
/// `process::builtin_submodule_key`'s cross-check (the spec type and its
/// fields are private to this module).
#[cfg(test)]
pub(crate) fn is_registered_submodule_key(key: &str) -> bool {
    ALL_SUBMODULE_SPECS.iter().any(|spec| spec.key == key)
}

/// Test-only: every submodule spec, for exhaustiveness checks (the production
/// `find_submodule` resolves through the registry, not an iterable array).
#[cfg(test)]
pub(super) const ALL_SUBMODULE_SPECS: &[&SubmoduleSpec] = &[
    &SUBMOD_VM,
    &SUBMOD_TIMERS,
    &SUBMOD_TIMERS_PROMISES,
    &SUBMOD_FS_PROMISES,
    &SUBMOD_READLINE_PROMISES,
    &SUBMOD_STREAM_PROMISES,
    &SUBMOD_STREAM_CONSUMERS,
    &SUBMOD_STREAM_WEB,
    &SUBMOD_HONO_JSX_SERVER,
    &SUBMOD_HONO_JSX_STREAMING,
    &SUBMOD_SYS,
    &SUBMOD_DIAGNOSTICS_CHANNEL,
    &SUBMOD_TRACE_EVENTS,
    &SUBMOD_TEST,
    &SUBMOD_TEST_REPORTERS,
];
#[no_mangle]
pub extern "C" fn js_node_submod_install_vm() {
    SUBMOD_REGISTRY[SubmodBucket::Vm as usize].store(
        &SUBMOD_VM as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_timers() {
    SUBMOD_REGISTRY[SubmodBucket::Timers as usize].store(
        &SUBMOD_TIMERS as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_timers_promises() {
    SUBMOD_REGISTRY[SubmodBucket::TimersPromises as usize].store(
        &SUBMOD_TIMERS_PROMISES as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_fs_promises() {
    SUBMOD_REGISTRY[SubmodBucket::FsPromises as usize].store(
        &SUBMOD_FS_PROMISES as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_readline_promises() {
    crate::object::install_readline_constructor_metadata();
    arm_readline_promises();
}
/// The registry store alone, for `js_nm_install_readline`: a program that names
/// `readline` can read `readline.promises`, whose exports live in this spec.
pub(crate) fn arm_readline_promises() {
    SUBMOD_REGISTRY[SubmodBucket::ReadlinePromises as usize].store(
        &SUBMOD_READLINE_PROMISES as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_stream_promises() {
    SUBMOD_REGISTRY[SubmodBucket::StreamPromises as usize].store(
        &SUBMOD_STREAM_PROMISES as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_stream_consumers() {
    SUBMOD_REGISTRY[SubmodBucket::StreamConsumers as usize].store(
        &SUBMOD_STREAM_CONSUMERS as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_stream_web() {
    SUBMOD_REGISTRY[SubmodBucket::StreamWeb as usize].store(
        &SUBMOD_STREAM_WEB as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_hono_jsx_server() {
    SUBMOD_REGISTRY[SubmodBucket::HonoJsxServer as usize].store(
        &SUBMOD_HONO_JSX_SERVER as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_hono_jsx_streaming() {
    SUBMOD_REGISTRY[SubmodBucket::HonoJsxStreaming as usize].store(
        &SUBMOD_HONO_JSX_STREAMING as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_sys() {
    // `node:sys` is a deprecated alias that delegates to the native `util` module.
    crate::object::js_nm_install_util();
    SUBMOD_REGISTRY[SubmodBucket::Sys as usize].store(
        &SUBMOD_SYS as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_diagnostics_channel() {
    SUBMOD_REGISTRY[SubmodBucket::DiagnosticsChannel as usize].store(
        &SUBMOD_DIAGNOSTICS_CHANNEL as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_trace_events() {
    SUBMOD_REGISTRY[SubmodBucket::TraceEvents as usize].store(
        &SUBMOD_TRACE_EVENTS as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_test() {
    SUBMOD_REGISTRY[SubmodBucket::Test as usize].store(
        &SUBMOD_TEST as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_test_reporters() {
    SUBMOD_REGISTRY[SubmodBucket::TestReporters as usize].store(
        &SUBMOD_TEST_REPORTERS as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_all() {
    js_node_submod_install_vm();
    js_node_submod_install_timers();
    js_node_submod_install_timers_promises();
    js_node_submod_install_fs_promises();
    js_node_submod_install_readline_promises();
    js_node_submod_install_stream_promises();
    js_node_submod_install_stream_consumers();
    js_node_submod_install_stream_web();
    js_node_submod_install_hono_jsx_server();
    js_node_submod_install_hono_jsx_streaming();
    js_node_submod_install_sys();
    js_node_submod_install_diagnostics_channel();
    js_node_submod_install_trace_events();
    // `node:test` only when the program installed it (`mod-node-test`).
    if let Some(install) = TEST_INSTALL_BUCKETS.get() {
        install();
    }
}
static SUBMOD_INSTALL_ALL_HOOK: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
#[no_mangle]
pub extern "C" fn js_node_submod_enable_install_all() {
    let f = std::hint::black_box(js_node_submod_install_all as extern "C" fn());
    SUBMOD_INSTALL_ALL_HOOK.store(f as *mut (), Ordering::Relaxed);
}
pub(crate) fn run_submod_install_all_hook() {
    let p = SUBMOD_INSTALL_ALL_HOOK.load(Ordering::Relaxed);
    if !p.is_null() {
        unsafe { std::mem::transmute::<*mut (), extern "C" fn()>(p)() };
    }
}

fn find_export(submod: &SubmoduleSpec, name: &str) -> Option<&'static ExportSpec> {
    submod.exports.iter().copied().find(|e| e.name == name)
}

// ----- singleton storage -----
//
// One AtomicI64 slot per thunk so concurrent first-use callers don't
// leak a closure. Stored in a thread_local Vec for simplicity — these
// singletons are allocated on first reach and live until process exit
// (they're root-marked by `scan_node_submodule_singleton_roots` below).

/// The singletons one submodule owns: its export thunks' closures by export
/// ordinal (the export's index in `exports`), its namespace object and its
/// CommonJS-style default object (for submodules whose `namespace.default` is
/// not the namespace itself). Null until first use. The submodule's bucket and
/// the export ordinal are the identity; no address of the spec or of its
/// names enters, so lookups do the same work wherever the link put them.
struct SubmodSlots {
    exports: Option<Box<Vec<*mut ClosureHeader>>>,
    namespace: *mut ObjectHeader,
    default_object: *mut ObjectHeader,
}
const EMPTY_SUBMOD_SLOTS: SubmodSlots = SubmodSlots {
    exports: None,
    namespace: std::ptr::null_mut(),
    default_object: std::ptr::null_mut(),
};

/// Read this thread's slots (`None` before its first submodule singleton).
fn with_submod_slots<R>(read: impl FnOnce(Option<&[SubmodSlots; SUBMOD_COUNT]>) -> R) -> R {
    SUBMOD_SINGLETONS.with(|m| read(m.borrow().as_deref()))
}

/// Write this thread's slots, allocating them on first use.
fn with_submod_slots_mut<R>(write: impl FnOnce(&mut [SubmodSlots; SUBMOD_COUNT]) -> R) -> R {
    SUBMOD_SINGLETONS.with(|m| {
        write(
            m.borrow_mut()
                .get_or_insert_with(|| Box::new([EMPTY_SUBMOD_SLOTS; SUBMOD_COUNT])),
        )
    })
}

/// Where one export's singleton lives. A getter builds it from constants, so
/// taking one member never names the sibling declarations.
#[derive(Clone, Copy)]
struct ExportSlot {
    key: &'static str,
    bucket: SubmodBucket,
    count: usize,
    ordinal: usize,
}

/// `names`' index of `name`, evaluated at compile time.
const fn const_ordinal(names: &[&str], name: &str) -> usize {
    let mut i = 0;
    while i < names.len() {
        let (a, b) = (names[i].as_bytes(), name.as_bytes());
        if a.len() == b.len() {
            let mut j = 0;
            while j < a.len() && a[j] == b[j] {
                j += 1;
            }
            if j == a.len() {
                return i;
            }
        }
        i += 1;
    }
    panic!("export is not in its submodule's list")
}

/// `export`'s ordinal in `submod.exports`.
fn export_ordinal(submod: &SubmoduleSpec, export: &ExportSpec) -> usize {
    submod
        .exports
        .iter()
        .position(|candidate| std::ptr::eq(*candidate, export))
        .expect("export does not belong to its submodule")
}

thread_local! {
    /// Per-submodule singleton slots, indexed by `SubmodBucket`, allocated on
    /// the thread's first submodule use. The thread-local itself is one
    /// pointer, so a thread that never touches a submodule pays no per-thread
    /// TLS for the slots (no init image to copy, nothing extra to zero).
    static SUBMOD_SINGLETONS: RefCell<Option<Box<[SubmodSlots; SUBMOD_COUNT]>>> =
        const { RefCell::new(None) };

    /// `node:timers/promises.scheduler` is a non-callable object with
    /// function-valued `wait` and `yield` properties.
    static TIMERS_PROMISES_SCHEDULER_OBJECT: RefCell<Option<*mut ObjectHeader>> =
        const { RefCell::new(None) };
}

// We also need a process-wide "any singleton allocated?" flag so the
// GC scanner can early-out without taking the thread_local borrow on
// every cycle. Using `AtomicI64` instead of `AtomicBool` so the scanner
// can also use it as a release fence against the thread_local writes.
static ANY_SINGLETON_ALLOCATED: AtomicI64 = AtomicI64::new(0);
static SYS_DEPRECATION_WARNED: AtomicI64 = AtomicI64::new(0);

pub(crate) fn emit_sys_deprecation_warning_once() {
    if SYS_DEPRECATION_WARNED
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let pid = crate::os::current_process_id();
    eprintln!(
        "(node:{pid}) [DEP0025] DeprecationWarning: sys is deprecated. Use `node:util` instead."
    );
    eprintln!("(Use `node --trace-deprecation ...` to show where the warning was created)");
}

fn sys_util_namespace_value() -> f64 {
    crate::object::js_create_native_module_namespace(b"util".as_ptr(), "util".len())
}

fn sys_util_default_value() -> f64 {
    unsafe {
        crate::object::js_native_module_property_by_name(
            b"util".as_ptr(),
            "util".len(),
            b"default".as_ptr(),
            "default".len(),
        )
    }
}

fn sys_util_export_value(name: &str) -> Option<f64> {
    if name == "default" {
        return Some(sys_util_default_value());
    }
    let value = unsafe {
        crate::object::js_native_module_property_by_name(
            b"util".as_ptr(),
            "util".len(),
            name.as_ptr(),
            name.len(),
        )
    };
    if value.to_bits() == crate::value::TAG_UNDEFINED {
        None
    } else {
        Some(value)
    }
}

fn fs_constants_namespace_value() -> f64 {
    unsafe {
        crate::object::js_native_module_property_by_name(
            b"fs".as_ptr(),
            "fs".len(),
            b"constants".as_ptr(),
            "constants".len(),
        )
    }
}

fn timers_promises_scheduler_value() -> f64 {
    TIMERS_PROMISES_SCHEDULER_OBJECT.with(|slot| {
        if let Some(cached) = *slot.borrow() {
            return f64::from_bits(JSValue::pointer(cached as *const u8).bits());
        }

        let obj = crate::realm_decl! { birth js_object_alloc(0, 2); realm { "node:timers/promises.scheduler", kind = namespace, members = { wait, attrs = writable | enumerable | configurable; yield, attrs = writable | enumerable | configurable; }; } };

        let wait = js_closure_alloc(
            crate::fn_info!(#[realm("node:timers/promises.scheduler.wait", provider = closure_alloc)] export crate::node_submodules::timers::timers_promises_scheduler_wait, 2; with_declared(2)),
            0,
        );

        set_named_value(
            obj,
            "wait",
            f64::from_bits(JSValue::pointer(wait as *const u8).bits()),
        );

        let yield_fn = js_closure_alloc(
            crate::fn_info!(#[realm("node:timers/promises.scheduler.yield", provider = closure_alloc)] export crate::node_submodules::timers::timers_promises_scheduler_yield, 0; with_declared(0)),
            0,
        );

        set_named_value(
            obj,
            "yield",
            f64::from_bits(JSValue::pointer(yield_fn as *const u8).bits()),
        );

        *slot.borrow_mut() = Some(obj);
        ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
        f64::from_bits(JSValue::pointer(obj as *const u8).bits())
    })
}

fn special_export_value(submod_key: &str, name: &str) -> Option<f64> {
    let value = match submod_key {
        "fs_promises" if name == "constants" => Some(fs_constants_namespace_value()),
        "timers_promises" if name == "scheduler" => Some(timers_promises_scheduler_value()),
        "stream_web"
            if matches!(
                name,
                "ReadableStream"
                    | "WritableStream"
                    | "TransformStream"
                    | "TextEncoderStream"
                    | "TextDecoderStream"
                    | "CompressionStream"
                    | "DecompressionStream"
            ) =>
        {
            Some(crate::object::js_get_global_this_builtin_value(
                name.as_ptr(),
                name.len(),
            ))
        }
        "test" => TEST_SPECIAL_EXPORT.get().and_then(|f| f(name)),
        _ => None,
    };
    if value.is_some() {
        ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
    }
    value
}

/// `node:test` wraps a handful of its exports so `test.skip`/`test.only`-style
/// chaining works. The runner is reached only through the slots below, which
/// the `mod-node-test` install fills, so a program that never imports
/// `node:test` links none of it (see `crate::feature_hooks` for why that also
/// unpins the JSON serializer).
fn maybe_decorate_test_export(
    submod_key: &'static str,
    export: &'static ExportSpec,
    key_name: &'static str,
    allocated: *mut ClosureHeader,
) -> Option<*mut ClosureHeader> {
    if submod_key == "test"
        && matches!(
            export.name,
            "default" | "test" | "suite" | "describe" | "it"
        )
    {
        let decorate = TEST_DECORATE.get()?;
        Some(decorate(
            allocated,
            key_name == "test",
            matches!(export.name, "suite" | "describe"),
        ))
    } else {
        None
    }
}

static TEST_INSTALL_BUCKETS: Hook<fn()> = Hook::empty();
static TEST_SPECIAL_EXPORT: Hook<fn(&str) -> Option<f64>> = Hook::empty();
static TEST_DECORATE: Hook<fn(*mut ClosureHeader, bool, bool) -> *mut ClosureHeader> =
    Hook::empty();
static TEST_SCAN_ROOTS: Hook<for<'a, 'b> fn(&'a mut crate::gc::RuntimeRootVisitor<'b>)> =
    Hook::empty();

fn install_test_buckets() {
    js_node_submod_install_test();
    js_node_submod_install_test_reporters();
}

/// The `mod-node-test` install: the `node:test` runner and its reporters.
pub(crate) fn install_node_test() {
    TEST_INSTALL_BUCKETS.set(install_test_buckets);
    TEST_SPECIAL_EXPORT.set(test::test_special_export_value);
    TEST_DECORATE.set(test::decorate_test_export);
    TEST_SCAN_ROOTS.set(test::scan_test_module_roots_mut);
    crate::feature_hooks::NODE_TEST_ROOT_SCANNER.set(test::runner::scan_node_test_runner_roots_mut);
    crate::gc::gc_register_named_mutable_root_scanner(
        NODE_TEST_ROOT_SCANNER_NAME,
        test::runner::scan_node_test_runner_roots_mut,
    );
}

pub(crate) const NODE_TEST_ROOT_SCANNER_NAME: &str =
    "crate::node_submodules::test::runner::scan_node_test_runner_roots_mut";

fn ensure_export_singleton(
    submod: &'static SubmoduleSpec,
    export: &'static ExportSpec,
) -> *mut ClosureHeader {
    let owner = if submod.key == "test" {
        let canonical = match export.name {
            "default" | "test" | "it" => "test",
            "suite" | "describe" => "suite",
            _ => export.name,
        };
        find_export(submod, canonical).unwrap_or(export)
    } else {
        export
    };
    let slot = ExportSlot {
        key: submod.key,
        bucket: submod.bucket,
        count: submod.exports.len(),
        ordinal: export_ordinal(submod, owner),
    };
    ensure_selected_export(slot, export, owner, attach_readline_if_installed)
}

unsafe fn attach_readline_if_installed(name: &str, value: f64, addr: usize) -> f64 {
    crate::object::nm_attach_lookup("readline").map_or(value, |attach| attach(name, value, addr))
}

#[inline(always)]
fn ensure_selected_export(
    slot: ExportSlot,
    export: &'static ExportSpec,
    owner: &'static ExportSpec,
    attach_readline: unsafe fn(&str, f64, usize) -> f64,
) -> *mut ClosureHeader {
    let (submod_key, key_name) = (slot.key, owner.name);
    let (bucket, ordinal) = (slot.bucket as usize, slot.ordinal);
    if let Some(cached) = export_singleton(bucket, ordinal) {
        return cached;
    }
    let allocated = js_closure_alloc(export.info, 0);
    if submod_key == "timers_promises" && export.name == "scheduler" {
        let wait = js_closure_alloc(
            crate::fn_info!(timers_promises_scheduler_wait, 2; with_declared(2)),
            0,
        );

        crate::closure::closure_set_dynamic_prop(
            allocated as usize,
            "wait",
            f64::from_bits(JSValue::pointer(wait as *const u8).bits()),
        );

        let yield_fn = js_closure_alloc(
            crate::fn_info!(timers_promises_scheduler_yield, 0; with_declared(0)),
            0,
        );

        crate::closure::closure_set_dynamic_prop(
            allocated as usize,
            "yield",
            f64::from_bits(JSValue::pointer(yield_fn as *const u8).bits()),
        );
    }
    let allocated = if submod_key == "readline_promises" && export.name == "Interface" {
        let scope = crate::gc::RuntimeHandleScope::new();
        let ctor = scope.root_raw_mut_ptr(allocated);
        // Reuse readline's imported decorator without pinning its parent
        // machinery from the always-linked submodule value resolver.
        {
            let attach = attach_readline;
            let value = ctor
                .with_mut_ptr(|p: *mut ClosureHeader| crate::value::js_nanbox_pointer(p as i64));
            unsafe {
                attach(
                    "InterfacePromises",
                    value,
                    crate::value::js_nanbox_get_pointer(value) as usize,
                );
            }
        }
        ctor.get_raw_mut_ptr()
    } else {
        allocated
    };
    let allocated = if submod_key == "trace_events" {
        let scope = crate::gc::RuntimeHandleScope::new();
        let allocated_handle = scope.root_raw_mut_ptr(allocated);
        crate::object::set_bound_native_closure_name(
            allocated_handle.get_raw_mut_ptr(),
            export.name,
        );
        crate::object::set_builtin_closure_length(
            allocated_handle.get_raw_mut_ptr::<ClosureHeader>() as usize,
            crate::closure::info_arity(export.info).unwrap_or(0),
        );
        crate::object::set_builtin_closure_non_constructable(
            allocated_handle.get_raw_mut_ptr::<ClosureHeader>() as usize,
        );
        allocated_handle.get_raw_mut_ptr()
    } else if let Some(decorated) =
        maybe_decorate_test_export(submod_key, export, key_name, allocated)
    {
        decorated
    } else {
        allocated
    };
    with_submod_slots_mut(|slots| {
        slots[bucket]
            .exports
            .get_or_insert_with(|| Box::new(vec![std::ptr::null_mut(); slot.count]))[ordinal] =
            allocated;
    });
    ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
    allocated
}

/// The closure already minted for export `ordinal` of submodule `bucket`.
fn export_singleton(bucket: usize, ordinal: usize) -> Option<*mut ClosureHeader> {
    with_submod_slots(|slots| {
        slots?[bucket]
            .exports
            .as_ref()?
            .get(ordinal)
            .copied()
            .filter(|p| !p.is_null())
    })
}

fn submodule_has_default_object(submod_key: &str) -> bool {
    matches!(
        submod_key,
        "diagnostics_channel"
            | "fs_promises"
            | "readline_promises"
            | "stream_consumers"
            | "stream_web"
            | "test_reporters"
            // `const nodeTimers = require('node:timers')` (Next.js's
            // fast-set-immediate extension) — without a default object the
            // binding read the TAG_TRUE sentinel, so member reads were
            // undefined and the `nodeTimers.setImmediate = patched`
            // monkey-patch threw at module init.
            | "timers"
            | "timers_promises"
    )
}

fn fs_promises_constants_value() -> f64 {
    unsafe {
        crate::object::js_native_module_property_by_name(
            b"fs".as_ptr(),
            "fs".len(),
            b"constants".as_ptr(),
            "constants".len(),
        )
    }
}

fn set_named_value(obj: *mut ObjectHeader, name: &str, value: f64) {
    let name_bytes = name.as_bytes();
    let name_header = js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
    crate::object::js_object_set_field_by_name(obj, name_header, value);
}

fn submodule_export_value(submod: &'static SubmoduleSpec, spec: &'static ExportSpec) -> f64 {
    if submod.key == "sys" {
        return sys_util_export_value(spec.name).unwrap_or_else(|| {
            let closure_ptr = ensure_export_singleton(submod, spec);
            f64::from_bits(JSValue::pointer(closure_ptr as *const u8).bits())
        });
    }
    if let Some(value) = special_export_value(submod.key, spec.name) {
        return value;
    }
    let closure_ptr = ensure_export_singleton(submod, spec);
    f64::from_bits(JSValue::pointer(closure_ptr as *const u8).bits())
}

fn submodule_default_object_value(submod: &'static SubmoduleSpec) -> Option<f64> {
    if !submodule_has_default_object(submod.key) {
        return None;
    }
    let bucket = submod.bucket as usize;
    let cached =
        with_submod_slots(|slots| slots.map_or(std::ptr::null_mut(), |s| s[bucket].default_object));
    if !cached.is_null() {
        return Some(f64::from_bits(JSValue::pointer(cached as *const u8).bits()));
    }

    let extra_fields = u32::from(submod.key == "fs_promises");
    let obj = crate::realm_decl! { birth js_object_alloc(0, submod.exports.len() as u32 + extra_fields); realm { "node:diagnostics_channel.default", kind = namespace, members = { tracingChannel = ref("node:diagnostics_channel.tracingChannel"), attrs = writable | enumerable | configurable; boundedChannel = ref("node:diagnostics_channel.boundedChannel"), attrs = writable | enumerable | configurable; channel = ref("node:diagnostics_channel.channel"), attrs = writable | enumerable | configurable; subscribe = ref("node:diagnostics_channel.subscribe"), attrs = writable | enumerable | configurable; unsubscribe = ref("node:diagnostics_channel.unsubscribe"), attrs = writable | enumerable | configurable; hasSubscribers = ref("node:diagnostics_channel.hasSubscribers"), attrs = writable | enumerable | configurable; Channel = ref("node:diagnostics_channel.Channel"), attrs = writable | enumerable | configurable; BoundedChannel = ref("node:diagnostics_channel.BoundedChannel"), attrs = writable | enumerable | configurable; }; "node:fs.promises", kind = namespace, members = { readFile = ref("node:fs/promises.readFile"), attrs = writable | enumerable | configurable; open = ref("node:fs/promises.open"), attrs = writable | enumerable | configurable; writeFile = ref("node:fs/promises.writeFile"), attrs = writable | enumerable | configurable; appendFile = ref("node:fs/promises.appendFile"), attrs = writable | enumerable | configurable; chmod = ref("node:fs/promises.chmod"), attrs = writable | enumerable | configurable; chown = ref("node:fs/promises.chown"), attrs = writable | enumerable | configurable; lchown = ref("node:fs/promises.lchown"), attrs = writable | enumerable | configurable; lchmod = ref("node:fs/promises.lchmod"), attrs = writable | enumerable | configurable; mkdir = ref("node:fs/promises.mkdir"), attrs = writable | enumerable | configurable; readdir = ref("node:fs/promises.readdir"), attrs = writable | enumerable | configurable; stat = ref("node:fs/promises.stat"), attrs = writable | enumerable | configurable; statfs = ref("node:fs/promises.statfs"), attrs = writable | enumerable | configurable; lstat = ref("node:fs/promises.lstat"), attrs = writable | enumerable | configurable; rm = ref("node:fs/promises.rm"), attrs = writable | enumerable | configurable; rmdir = ref("node:fs/promises.rmdir"), attrs = writable | enumerable | configurable; unlink = ref("node:fs/promises.unlink"), attrs = writable | enumerable | configurable; rename = ref("node:fs/promises.rename"), attrs = writable | enumerable | configurable; copyFile = ref("node:fs/promises.copyFile"), attrs = writable | enumerable | configurable; cp = ref("node:fs/promises.cp"), attrs = writable | enumerable | configurable; truncate = ref("node:fs/promises.truncate"), attrs = writable | enumerable | configurable; utimes = ref("node:fs/promises.utimes"), attrs = writable | enumerable | configurable; lutimes = ref("node:fs/promises.lutimes"), attrs = writable | enumerable | configurable; link = ref("node:fs/promises.link"), attrs = writable | enumerable | configurable; symlink = ref("node:fs/promises.symlink"), attrs = writable | enumerable | configurable; readlink = ref("node:fs/promises.readlink"), attrs = writable | enumerable | configurable; realpath = ref("node:fs/promises.realpath"), attrs = writable | enumerable | configurable; mkdtemp = ref("node:fs/promises.mkdtemp"), attrs = writable | enumerable | configurable; mkdtempDisposable = ref("node:fs/promises.mkdtempDisposable"), attrs = writable | enumerable | configurable; opendir = ref("node:fs/promises.opendir"), attrs = writable | enumerable | configurable; glob = ref("node:fs/promises.glob"), attrs = writable | enumerable | configurable; watch = ref("node:fs/promises.watch"), attrs = writable | enumerable | configurable; access = ref("node:fs/promises.access"), attrs = writable | enumerable | configurable; constants = ref("node:fs.constants"), attrs = writable | enumerable | configurable; }; "node:stream/consumers.default", kind = namespace, members = { text = ref("node:stream/consumers.text"), attrs = writable | enumerable | configurable; json = ref("node:stream/consumers.json"), attrs = writable | enumerable | configurable; buffer = ref("node:stream/consumers.buffer"), attrs = writable | enumerable | configurable; arrayBuffer = ref("node:stream/consumers.arrayBuffer"), attrs = writable | enumerable | configurable; bytes = ref("node:stream/consumers.bytes"), attrs = writable | enumerable | configurable; blob = ref("node:stream/consumers.blob"), attrs = writable | enumerable | configurable; }; "node:stream/web.default", kind = namespace, members = { ReadableStream = ref(ReadableStream), attrs = writable | enumerable | configurable; ReadableStreamDefaultReader = ref("node:stream/web.ReadableStreamDefaultReader"), attrs = writable | enumerable | configurable; ReadableStreamBYOBReader = ref("node:stream/web.ReadableStreamBYOBReader"), attrs = writable | enumerable | configurable; ReadableStreamDefaultController = ref("node:stream/web.ReadableStreamDefaultController"), attrs = writable | enumerable | configurable; ReadableByteStreamController = ref("node:stream/web.ReadableByteStreamController"), attrs = writable | enumerable | configurable; ReadableStreamBYOBRequest = ref("node:stream/web.ReadableStreamBYOBRequest"), attrs = writable | enumerable | configurable; WritableStream = ref(WritableStream), attrs = writable | enumerable | configurable; WritableStreamDefaultWriter = ref("node:stream/web.WritableStreamDefaultWriter"), attrs = writable | enumerable | configurable; WritableStreamDefaultController = ref("node:stream/web.WritableStreamDefaultController"), attrs = writable | enumerable | configurable; TransformStream = ref(TransformStream), attrs = writable | enumerable | configurable; TransformStreamDefaultController = ref("node:stream/web.TransformStreamDefaultController"), attrs = writable | enumerable | configurable; ByteLengthQueuingStrategy = ref("node:stream/web.ByteLengthQueuingStrategy"), attrs = writable | enumerable | configurable; CountQueuingStrategy = ref("node:stream/web.CountQueuingStrategy"), attrs = writable | enumerable | configurable; TextEncoderStream = ref(TextEncoderStream), attrs = writable | enumerable | configurable; TextDecoderStream = ref(TextDecoderStream), attrs = writable | enumerable | configurable; CompressionStream = ref(CompressionStream), attrs = writable | enumerable | configurable; DecompressionStream = ref(DecompressionStream), attrs = writable | enumerable | configurable; }; "node:test/reporters.default", kind = namespace, members = { spec, attrs = writable | enumerable | configurable; tap, attrs = writable | enumerable | configurable; dot, attrs = writable | enumerable | configurable; junit, attrs = writable | enumerable | configurable; lcov, attrs = writable | enumerable | configurable; }; "node:timers.default", kind = namespace, members = { setTimeout, attrs = writable | enumerable | configurable; setInterval, attrs = writable | enumerable | configurable; setImmediate, attrs = writable | enumerable | configurable; clearTimeout, attrs = writable | enumerable | configurable; clearInterval, attrs = writable | enumerable | configurable; clearImmediate, attrs = writable | enumerable | configurable; }; "node:timers.promises", kind = namespace, members = { setTimeout = ref("node:timers/promises.setTimeout"), attrs = writable | enumerable | configurable; setImmediate = ref("node:timers/promises.setImmediate"), attrs = writable | enumerable | configurable; setInterval = ref("node:timers/promises.setInterval"), attrs = writable | enumerable | configurable; scheduler = ref("node:timers/promises.scheduler"), attrs = writable | enumerable | configurable; }; } };
    for spec in submod.exports {
        set_named_value(obj, spec.name, submodule_export_value(submod, spec));
    }
    if submod.key == "fs_promises" {
        set_named_value(obj, "constants", fs_promises_constants_value());
    }

    with_submod_slots_mut(|slots| slots[bucket].default_object = obj);
    ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
    Some(f64::from_bits(JSValue::pointer(obj as *const u8).bits()))
}

/// The existing CommonJS export value of a registry-backed builtin. Default
/// imports and computed require must agree on this value; the ESM namespace
/// remains a separate module object with a default binding pointing here.
pub(crate) fn commonjs_value(key: &str) -> f64 {
    run_submod_install_all_hook();
    let Some(submod) = find_submodule(key) else {
        return crate::object::js_unresolved_namespace_stub();
    };
    if let Some(value) = submodule_default_object_value(submod) {
        return value;
    }
    if let Some(value) = special_export_value(key, "default") {
        return value;
    }
    if let Some(export) = find_export(submod, "default") {
        return submodule_export_value(submod, export);
    }
    if let Some(value) = native_namespace_as_default(submod) {
        return value;
    }
    let obj = ensure_namespace_singleton(submod);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

pub(crate) fn is_diagnostics_channel_constructor_value(value: f64) -> bool {
    let js_value = JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        return false;
    }
    let ptr = js_value.as_pointer::<ClosureHeader>() as *mut ClosureHeader;
    let Some(submod) = find_submodule("diagnostics_channel") else {
        return false;
    };
    let Some(export) = find_export(submod, "Channel") else {
        return false;
    };
    export_singleton(submod.bucket as usize, export_ordinal(submod, export)) == Some(ptr)
}

pub(crate) fn is_diagnostics_bounded_channel_constructor_value(value: f64) -> bool {
    let js_value = JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        return false;
    }
    let ptr = js_value.as_pointer::<ClosureHeader>() as *mut ClosureHeader;
    let Some(submod) = find_submodule("diagnostics_channel") else {
        return false;
    };
    let Some(export) = find_export(submod, "BoundedChannel") else {
        return false;
    };
    export_singleton(submod.bucket as usize, export_ordinal(submod, export)) == Some(ptr)
}

fn ensure_namespace_singleton(submod: &'static SubmoduleSpec) -> *mut ObjectHeader {
    let bucket = submod.bucket as usize;
    let cached =
        with_submod_slots(|slots| slots.map_or(std::ptr::null_mut(), |s| s[bucket].namespace));
    if !cached.is_null() {
        return cached;
    }
    // Allocate a fresh object with one inline slot per known export;
    // the dynamic-property path in `js_object_set_field_by_name` will
    // grow it if needed.
    let field_count = submod.exports.len() as u32
        + u32::from(submod.key == "fs_promises")
        + u32::from(submodule_has_default_object(submod.key));
    let obj = crate::realm_decl! { birth js_object_alloc(0, field_count); realm { "node:stream.promises", kind = namespace, members = { pipeline = ref("node:stream/promises.pipeline"), attrs = writable | enumerable | configurable; finished = ref("node:stream/promises.finished"), attrs = writable | enumerable | configurable; default = ref("node:stream.promises"), attrs = writable | enumerable | configurable; }; } };
    // Populate fields. Each export's value is the singleton closure
    // pointer NaN-boxed as POINTER. We route through
    // `js_object_set_field_by_name` so the keys array gets built up
    // identically to what user code's literal object init would
    // produce — that's what `js_object_keys` / spread / Reflect.ownKeys
    // walks at runtime.
    for spec in submod.exports {
        set_named_value(obj, spec.name, submodule_export_value(submod, spec));
    }
    if submod.key == "fs_promises" {
        set_named_value(obj, "constants", fs_promises_constants_value());
    }
    if submod.key == "stream_promises" {
        let value = value_from_ptr(obj as *const u8);
        let name = b"default";
        let name_header = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, name_header, value);
    }
    if submod.key == "timers" {
        let value = crate::object::timers_promises_parent_namespace();
        let name = b"promises";
        let name_header = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, name_header, value);
    }
    if submod.key == "trace_events" {
        for spec in submod.exports {
            crate::object::set_property_attrs(
                obj as usize,
                spec.name.to_string(),
                PropertyAttrs::new(true, true, false),
            );
        }
        let default_obj = crate::realm_decl! { birth js_object_alloc(0, submod.exports.len() as u32); realm { "node:trace_events.default", kind = namespace, members = { createTracing, attrs = writable | enumerable | configurable; getEnabledCategories, attrs = writable | enumerable | configurable; }; } };
        for spec in submod.exports {
            let closure_ptr = ensure_export_singleton(submod, spec);
            let value = f64::from_bits(JSValue::pointer(closure_ptr as *const u8).bits());
            let name_bytes = spec.name.as_bytes();
            let name_header = js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
            crate::object::js_object_set_field_by_name(default_obj, name_header, value);
        }
        let name = b"default";
        let name_header = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(
            obj,
            name_header,
            f64::from_bits(JSValue::pointer(default_obj as *const u8).bits()),
        );
        crate::object::set_property_attrs(
            obj as usize,
            "default".to_string(),
            PropertyAttrs::new(true, true, false),
        );
    }
    if let Some(default_value) = submodule_default_object_value(submod) {
        set_named_value(obj, "default", default_value);
    }
    with_submod_slots_mut(|slots| slots[bucket].namespace = obj);
    ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
    obj
}

/// GC root scanner: pin every (export-singleton, namespace-singleton)
/// allocated by this module against the next sweep. Wired up from
/// `gc::gc_init`.
pub fn scan_node_submodule_singleton_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_node_submodule_singleton_roots_mut(&mut visitor);
}

pub fn scan_node_submodule_singleton_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    DIAG_PENDING_UNCAUGHT.with(|pending| {
        for (err, _) in pending.borrow_mut().iter_mut() {
            visitor.visit_nanbox_f64_slot(err);
        }
    });
    // Symbol keys duplicate the structurally rooted DIAG_CHANNELS names.
    // This is a rewrite-only identity index and can exist independently of
    // the export/namespace singleton caches guarded below.
    diagnostics::scan_diagnostics_channel_key_roots_mut(visitor);
    if ANY_SINGLETON_ALLOCATED.load(Ordering::Acquire) == 0 {
        return;
    }
    SUBMOD_SINGLETONS.with(|m| {
        for slots in m.borrow_mut().iter_mut().flat_map(|all| all.iter_mut()) {
            for closure_ptr in slots.exports.iter_mut().flat_map(|e| e.iter_mut()) {
                if closure_ptr.is_null() {
                    continue;
                }
                visitor.visit_raw_mut_ptr_slot(closure_ptr);
            }
            for obj_ptr in [&mut slots.namespace, &mut slots.default_object] {
                if !obj_ptr.is_null() {
                    visitor.visit_raw_mut_ptr_slot(obj_ptr);
                }
            }
        }
    });
    TIMERS_PROMISES_SCHEDULER_OBJECT.with(|slot| {
        if let Some(ptr) = slot.borrow_mut().as_mut() {
            visitor.visit_raw_mut_ptr_slot(ptr);
        }
    });
    // #906 follow-up: the no-op closure shared by every TracingChannel /
    // Channel stub field also needs pinning against the next sweep. The
    // returned stub objects themselves are caller-owned (we don't cache
    // them) so they're traced through normal allocator roots.
    DIAG_NOOP_CLOSURE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(ptr) = slot.as_mut() {
            visitor.visit_raw_mut_ptr_slot(ptr);
        }
    });
    DIAG_CHANNELS.with(|m| {
        for state in m.borrow_mut().values_mut() {
            visitor.visit_nanbox_f64_slot(&mut state.name);
            visitor.visit_raw_mut_ptr_slot(&mut state.obj);
            for subscriber in &mut state.subscribers {
                visitor.visit_nanbox_f64_slot(subscriber);
            }
            for (store, transform) in &mut state.stores {
                visitor.visit_nanbox_f64_slot(store);
                if let StoreTransform::Callable(t) = transform {
                    visitor.visit_nanbox_f64_slot(t);
                }
            }
        }
    });
    DIAG_TRACES.with(|m| {
        for trace in m.borrow_mut().values_mut() {
            visitor.visit_raw_mut_ptr_slot(&mut trace.obj);
        }
    });
    diagnostics_tail::scan_diagnostics_tail_roots_mut(visitor);
    trace_events::scan_trace_events_roots_mut(visitor);
    if let Some(scan) = TEST_SCAN_ROOTS.get() {
        scan(visitor);
    }
}

#[cfg(test)]
pub(crate) fn test_seed_node_submodule_roots(
    closure: *mut ClosureHeader,
    namespace: *mut ObjectHeader,
    diag_noop: *mut ClosureHeader,
) {
    SUBMOD_SINGLETONS.with(|m| *m.borrow_mut() = None);
    with_submod_slots_mut(|slots| {
        slots[0].exports = Some(Box::new(vec![closure]));
        slots[0].namespace = namespace;
    });
    DIAG_NOOP_CLOSURE.with(|slot| {
        *slot.borrow_mut() = Some(diag_noop);
    });
    DIAG_PENDING_UNCAUGHT.with(|pending| {
        let mut pending = pending.borrow_mut();
        pending.clear();
        pending.push((
            f64::from_bits(JSValue::pointer(namespace as *const u8).bits()),
            None,
        ));
    });
    ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
}

#[cfg(test)]
pub(crate) fn test_node_submodule_roots() -> (usize, usize, usize, u64) {
    let closure = export_singleton(0, 0).map_or(0, |ptr| ptr as usize);
    let namespace = with_submod_slots(|slots| slots.map_or(0, |s| s[0].namespace as usize));
    let diag =
        DIAG_NOOP_CLOSURE.with(|slot| slot.borrow().as_ref().map(|ptr| *ptr as usize).unwrap_or(0));
    let pending = DIAG_PENDING_UNCAUGHT.with(|pending| {
        pending
            .borrow_mut()
            .pop()
            .map(|(value, _)| value.to_bits())
            .unwrap_or(0)
    });
    (closure, namespace, diag, pending)
}

// ----- FFI entry points -----
//
// `submod_key_ptr` / `name_ptr` are `*const u8` pointers + lengths
// rather than NUL-terminated strings so codegen can hand off the raw
// bytes from emitted IR (already produced as `private constant
// [N x i8]` arrays via `emit_string_literal`).

/// Returns a NaN-boxed export singleton for the given
/// `(submodule, export)` pair. Falls back to NaN-boxed `TAG_TRUE`
/// (preserving the pre-#841 sentinel) if no matching entry is found —
/// this keeps any not-yet-listed export's behavior unchanged, so
/// later additions to `SUBMODULES` are strictly additive.
///
/// # Safety
///
/// The `submod_key_ptr` / `name_ptr` arguments must point to valid UTF-8
/// byte sequences of the indicated length, and remain alive for the
/// duration of this call.
#[no_mangle]
pub unsafe extern "C" fn js_node_submodule_export_as_function(
    submod_key_ptr: *const u8,
    submod_key_len: u32,
    name_ptr: *const u8,
    name_len: u32,
) -> f64 {
    let submod_bytes = std::slice::from_raw_parts(submod_key_ptr, submod_key_len as usize);
    let name_bytes = std::slice::from_raw_parts(name_ptr, name_len as usize);
    let submod_key = match std::str::from_utf8(submod_bytes) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(JSValue::bool(true).bits()),
    };
    let name = match std::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(JSValue::bool(true).bits()),
    };
    let submod = match find_submodule(submod_key) {
        Some(s) => s,
        None => return f64::from_bits(JSValue::bool(true).bits()),
    };
    if submod.key == "sys" {
        emit_sys_deprecation_warning_once();
        if let Some(value) = sys_util_export_value(name) {
            return value;
        }
    }
    if submod.key == "stream_promises" && name == "default" {
        let obj = ensure_namespace_singleton(submod);
        return f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    }
    if submod.key == "fs_promises" && name == "constants" {
        return fs_promises_constants_value();
    }
    if submod.key == "trace_events" && name == "default" {
        let obj = ensure_namespace_singleton(submod);
        let name_header = js_string_from_bytes(b"default".as_ptr(), 7);
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, name_header);
        return value;
    }
    if submod.key == "test_reporters" && name == "default" {
        if let Some(value) = submodule_default_object_value(submod) {
            return value;
        }
    }
    if name == "default" {
        if let Some(value) = submodule_default_object_value(submod) {
            return value;
        }
        // A submodule without a default export object: `import x from "node:<sub>"`
        // binds the namespace itself (CommonJS `module.exports`), never the
        // unknown-export sentinel (registry-backed modules only). It is the one `require("<sub>")`
        // and `import * as` hand out, so identity holds across the forms.
        if let Some(value) = native_namespace_as_default(submod) {
            return value;
        }
    }
    if submod.key == "timers" && name == "promises" {
        return crate::object::timers_promises_parent_namespace();
    }
    if let Some(value) = special_export_value(submod.key, name) {
        return value;
    }
    let export = match find_export(submod, name) {
        Some(e) => e,
        None => return f64::from_bits(JSValue::bool(true).bits()),
    };
    let closure_ptr = ensure_export_singleton(submod, export);
    f64::from_bits(JSValue::pointer(closure_ptr as *const u8).bits())
}

/// Returns a namespace-member value for a known Node submodule namespace import.
///
/// This differs from `js_node_submodule_export_as_function`: direct named-import
/// fallback behavior still uses the historical `TAG_TRUE` sentinel for
/// unlisted exports, but namespace property reads must behave like ordinary JS
/// objects and return `undefined` for absent properties.
#[no_mangle]
pub unsafe extern "C" fn js_node_submodule_namespace_member(
    submod_key_ptr: *const u8,
    submod_key_len: u32,
    name_ptr: *const u8,
    name_len: u32,
) -> f64 {
    // Devirt: dynamically-resolved submodules (require / getBuiltinModule / a
    // `fs.promises`-style property access) had no codegen install — run the
    // install-all hook so the registry is populated. Null/no-op unless armed.
    run_submod_install_all_hook();
    let submod_bytes = std::slice::from_raw_parts(submod_key_ptr, submod_key_len as usize);
    let name_bytes = std::slice::from_raw_parts(name_ptr, name_len as usize);
    let submod_key = match std::str::from_utf8(submod_bytes) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    let name = match std::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    let submod = match find_submodule(submod_key) {
        Some(s) => s,
        None => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    if submod.key == "sys" {
        emit_sys_deprecation_warning_once();
        if name == "default" {
            return sys_util_default_value();
        }
        return unsafe {
            crate::object::js_native_module_property_by_name(
                b"util".as_ptr(),
                "util".len(),
                name.as_ptr(),
                name.len(),
            )
        };
    }
    if submod.key == "stream_promises" && name == "default" {
        let obj = ensure_namespace_singleton(submod);
        return f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    }
    if submod.key == "fs_promises" && name == "constants" {
        return fs_promises_constants_value();
    }
    if submod.key == "trace_events" && name == "default" {
        let obj = ensure_namespace_singleton(submod);
        let name_header = js_string_from_bytes(b"default".as_ptr(), 7);
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, name_header);
        return value;
    }
    if submod.key == "test_reporters" && name == "default" {
        if let Some(value) = submodule_default_object_value(submod) {
            return value;
        }
    }
    if name == "default" {
        if let Some(value) = submodule_default_object_value(submod) {
            return value;
        }
        if let Some(value) = native_namespace_as_default(submod) {
            return value;
        }
    }
    if submod.key == "timers" && name == "promises" {
        return crate::object::timers_promises_parent_namespace();
    }
    if let Some(value) = special_export_value(submod.key, name) {
        return value;
    }
    let export = match find_export(submod, name) {
        Some(e) => e,
        None => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    let closure_ptr = ensure_export_singleton(submod, export);
    f64::from_bits(JSValue::pointer(closure_ptr as *const u8).bits())
}

crate::realm_decl! { module_provider /// Returns a NaN-boxed namespace stub object for the given submodule.
/// Each known named export of that submodule is exposed as an own
/// property on the object whose value is the export singleton
/// produced by `js_node_submodule_export_as_function`. Falls back to
/// `js_unresolved_namespace_stub` (the empty-object stub Perry already
/// hands out for unknown namespace imports) if `submod_key` doesn't
/// match a known submodule.
///
/// # Safety
///
/// Same constraints as `js_node_submodule_export_as_function`.
#[no_mangle]
pub unsafe extern "C" fn js_node_submodule_namespace(
    submod_key_ptr: *const u8,
    submod_key_len: u32,
) -> f64 {
    run_submod_install_all_hook();
    let submod_bytes = std::slice::from_raw_parts(submod_key_ptr, submod_key_len as usize);
    let submod_key = match std::str::from_utf8(submod_bytes) {
        Ok(s) => s,
        Err(_) => return crate::object::js_unresolved_namespace_stub(),
    };
    let submod = match find_submodule(submod_key) {
        Some(s) => s,
        None => return crate::object::js_unresolved_namespace_stub(),
    };
    if submod.key == "sys" {
        emit_sys_deprecation_warning_once();
        return sys_util_namespace_value();
    }
    let obj = ensure_namespace_singleton(submod);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
} realm { "node:diagnostics_channel", members = { BoundedChannel; Channel; boundedChannel; channel; default; hasSubscribers; subscribe; tracingChannel; unsubscribe; }, births = [Array, Error, Function, Object]; "node:fs/promises", members = { access; appendFile; chmod; chown; constants = ref("node:fs.constants"); copyFile; cp; default = ref("node:fs.promises"); glob; lchmod; lchown; link; lstat; lutimes; mkdir; mkdtemp; mkdtempDisposable; open; opendir; readFile; readdir; readlink; realpath; rename; rm; rmdir; stat; statfs; symlink; truncate; unlink; utimes; watch; writeFile; }, births = [Array, ArrayBuffer, Buffer, Error, Function, Object, Promise, ReadableStream]; "node:readline/promises", members = { Interface; Readline; createInterface; default = ref("node:readline.promises"); }, births = [Array, Error, Function, Object, Promise]; "node:stream/consumers", members = { arrayBuffer; blob; buffer; bytes; default; json; text; }, births = [Array, ArrayBuffer, Blob, Buffer, Error, Function, Object, Promise, ReadableStream]; "node:stream/promises", members = { default = ref("node:stream.promises"); finished; pipeline; }, births = [Array, ArrayBuffer, Buffer, Error, Function, Object, Promise, ReadableStream, WritableStream]; "node:stream/web", members = { ByteLengthQueuingStrategy; CompressionStream = ref(CompressionStream); CountQueuingStrategy; DecompressionStream = ref(DecompressionStream); ReadableByteStreamController; ReadableStream = ref(ReadableStream); ReadableStreamBYOBReader; ReadableStreamBYOBRequest; ReadableStreamDefaultController; ReadableStreamDefaultReader; TextDecoderStream = ref(TextDecoderStream); TextEncoderStream = ref(TextEncoderStream); TransformStream = ref(TransformStream); TransformStreamDefaultController; WritableStream = ref(WritableStream); WritableStreamDefaultController; WritableStreamDefaultWriter; default; }, births = [Array, ArrayBuffer, Buffer, Error, Function, Object, Promise]; "node:sys", members = { MIMEParams; MIMEType; TextDecoder = ref(TextDecoder); TextEncoder = ref(TextEncoder); _errnoException; _exceptionWithHostPort; _extend; aborted; callbackify; convertProcessSignalToExitCode; debug; debuglog = ref("node:sys.debug"); default; deprecate; diff; format; formatWithOptions; getCallSites; getSystemErrorMap; getSystemErrorMessage; getSystemErrorName; inherits; inspect; isArray; isDeepStrictEqual; parseArgs; parseEnv; promisify; setTraceSigInt; stripVTControlCharacters; styleText; toUSVString; transferableAbortController; transferableAbortSignal; types; }, births = [Array, Error, Function, Object]; "node:test", members = { after; afterEach; assert; before; beforeEach; default; describe; expectFailure; it = ref("node:test.default"); mock; only; run; skip; snapshot; suite = ref("node:test.describe"); test = ref("node:test.default"); todo; }, births = [Array, Error, Function, Object]; "node:test/reporters", members = { default; dot = undefined; junit = undefined; lcov = undefined; spec = undefined; tap = undefined; }, births = [Array, Error, Function, Object]; "node:timers", members = { clearImmediate; clearInterval; clearTimeout; default; promises; setImmediate; setInterval; setTimeout; }, births = [AbortSignal, Array, Error, Function, Object, Promise]; "node:timers/promises", members = { default = ref("node:timers.promises"); scheduler; setImmediate; setInterval; setTimeout; }, births = [AbortSignal, Array, Error, Function, Object, Promise]; "node:trace_events", members = { default; }, births = [Array, Error, Function, Object]; } }

#[cfg(test)]
mod tests;

/// Install the provider's ReadableStream.from on the actual constructor
/// shapes, with no property-read dispatch or provider side table.
/// # Safety
/// `info` must have static lifetime and describe a one-argument JS method.
#[no_mangle]
pub unsafe extern "C" fn js_install_readable_stream_from(info: *const JsFunctionInfo) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let method = scope.root_raw_mut_ptr(js_closure_alloc(&*info, 0));
    method.with_mut_ptr(|p| crate::object::set_bound_native_closure_name(p, "from"));
    method.with_mut_ptr(|p: *mut ClosureHeader| {
        crate::object::set_builtin_closure_length(p as usize, 1);
        crate::object::set_builtin_closure_non_constructable(p as usize);
    });
    // The module export aliases the global constructor. Decorate that one
    // shape without arming an unimported submodule or allocating a namespace.
    let ctor = scope.root_nanbox_f64(crate::object::js_get_global_this_builtin_value(
        b"ReadableStream".as_ptr(),
        14,
    ));
    let addr = crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64()) as usize;
    crate::closure::closure_set_dynamic_prop(
        addr,
        "from",
        method.with_mut_ptr(|p: *mut ClosureHeader| crate::value::js_nanbox_pointer(p as i64)),
    );
    crate::object::set_builtin_property_attrs(
        addr,
        "from".to_string(),
        crate::object::PropertyAttrs::new(true, true, true),
    );
}
