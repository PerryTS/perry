//! Link-time feature installation for the runtime.
//!
//! The runtime's always-live hubs — the `globalThis` builder, the native-module
//! member lookup, the generic operators — must not name an optional subsystem
//! (the `eval` parser, Intl/ICU, Temporal, the Bun CLI utilities, …) directly.
//! A direct reference keeps that subsystem in every program linked against the
//! prebuilt full-feature `libperry_runtime.a`, which is what an installed perry
//! links when it has no workspace to rebuild from.
//!
//! Each optional feature instead fills [`Hook`] slots from its own
//! `js_runtime_install_<feature>` entry point, and a hub calls through the slot;
//! an empty slot takes the path the hub's `#[cfg(not(feature))]` branch takes
//! in a build without the feature. perry's link step generates an object whose
//! static constructor registers an installer through
//! [`js_runtime_register_feature_installer`]; [`crate::gc::js_gc_init`] runs it
//! before any user code. The installer names the program's runtime features on
//! a prebuilt-archive link and [`js_runtime_install_compiled`] otherwise (an
//! auto-optimized archive already holds exactly the needed features).
//!
//! perry-stdlib uses the same [`Hook`] for its own hubs
//! (`perry_stdlib::common::feature_hooks`).

use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};

/// One function-pointer slot. `F` must be a plain `fn` pointer type.
pub struct Hook<F: Copy> {
    bits: AtomicUsize,
    _f: PhantomData<F>,
}

impl<F: Copy> Hook<F> {
    pub const fn empty() -> Self {
        Self {
            bits: AtomicUsize::new(0),
            _f: PhantomData,
        }
    }

    #[inline]
    pub fn set(&self, f: F) {
        const { assert!(std::mem::size_of::<F>() == std::mem::size_of::<usize>()) };
        // SAFETY: `F` is a fn pointer of pointer size (asserted above).
        let bits: usize = unsafe { std::mem::transmute_copy(&f) };
        self.bits.store(bits, Ordering::Release);
    }

    /// The slot's value, without the unit-test fallback of [`Hook::get`]:
    /// for slots read while the runtime itself initializes (`gc_init`), where
    /// installing everything would re-enter it.
    #[inline]
    pub fn peek(&self) -> Option<F> {
        let bits = self.bits.load(Ordering::Acquire);
        if bits == 0 {
            None
        } else {
            // SAFETY: only `set` stores non-zero bits, and it stores an `F`.
            Some(unsafe { std::mem::transmute_copy(&bits) })
        }
    }

    #[inline]
    pub fn get(&self) -> Option<F> {
        #[cfg_attr(not(test), allow(unused_mut))]
        let mut bits = self.bits.load(Ordering::Acquire);
        // Unit tests reach hubs without the perry link step or `js_gc_init`,
        // so an empty slot installs everything compiled (the pre-hook
        // behavior) and is read again. Test builds only: a shipped archive
        // must not reference `js_runtime_install_compiled` from here.
        #[cfg(test)]
        if bits == 0 {
            install_compiled_for_tests();
            bits = self.bits.load(Ordering::Acquire);
        }
        if bits == 0 {
            None
        } else {
            // SAFETY: only `set` stores non-zero bits, and it stores an `F`.
            Some(unsafe { std::mem::transmute_copy(&bits) })
        }
    }
}

#[cfg(test)]
fn install_compiled_for_tests() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        js_runtime_install_compiled();
        crate::object::js_nm_install_events();
        crate::object::js_nm_install_fs();
    });
}

/// Run `$body` once per process, so installs are idempotent.
#[macro_export]
#[doc(hidden)]
macro_rules! perry_install_once {
    ($body:block) => {{
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| $body);
    }};
}

/// The installer the program's generated object registered.
static FEATURE_INSTALLER: Hook<extern "C" fn()> = Hook::empty();

/// Called from a static constructor in the object perry's link step generates.
/// Only stores the pointer: it runs before `main`; installing happens from
/// [`crate::gc::js_gc_init`].
#[no_mangle]
pub extern "C" fn js_runtime_register_feature_installer(installer: extern "C" fn()) {
    FEATURE_INSTALLER.set(installer);
}

/// Run the registered installer; called at the end of `js_gc_init`.
///
/// With no registration nothing optional is installed. This must not name
/// [`js_runtime_install_compiled`] outside tests: `js_gc_init` is live in every
/// program, so a fallback reference here would pin every feature again. The
/// crate's own unit tests keep the pre-hook behavior.
///
/// The installer is always the one `perry_codegen::stubs::generate_feature_installer_object`
/// generates, which only calls `js_runtime_install_*` entry points.
/// `scripts/gc_call_effects/seeds.txt` delegates this indirect call to those
/// entry points (and forbids any archive code from registering an installer),
/// which is what keeps `js_gc_init` Leaf in the GC call-effects tables.
/// `#[inline(never)]` keeps the indirect call in this named symbol.
#[inline(never)]
pub(crate) fn run_feature_installer() {
    if let Some(installer) = FEATURE_INSTALLER.get() {
        installer();
        return;
    }
    #[cfg(test)]
    js_runtime_install_compiled();
}

/// Install every optional runtime feature this archive was compiled with
/// (its Cargo features).
#[no_mangle]
pub extern "C" fn js_runtime_install_compiled() {
    js_runtime_install_async_hooks();
    js_runtime_install_thread();
    js_runtime_install_date();
    js_runtime_install_perf_hooks();
    js_runtime_install_os();
    crate::wasi_hook_slots::install();
    js_runtime_install_messaging();
    js_runtime_install_event_target();
    js_runtime_install_typedarray();
    #[cfg(feature = "dyn-eval")]
    js_runtime_install_dyn_eval();
    #[cfg(feature = "bun-cli-utils")]
    js_runtime_install_bun_cli_utils();
    #[cfg(feature = "intl-namespace")]
    js_runtime_install_intl_namespace();
    #[cfg(feature = "temporal")]
    js_runtime_install_temporal();
    #[cfg(feature = "intl-datetime")]
    js_runtime_install_intl_datetime();
    #[cfg(feature = "regex-engine")]
    js_runtime_install_regex_engine();
    #[cfg(feature = "url-engine")]
    js_runtime_install_url_engine();
    js_runtime_install_string_normalize();
    js_runtime_install_intl_segmenter();
    js_runtime_install_intl_locale();
    js_runtime_install_turnloop();
    js_runtime_install_bigint();
    js_runtime_install_proxy();
    js_runtime_install_global_math();
    js_runtime_install_global_json();
    js_runtime_install_global_reflect();
    js_runtime_install_global_atomics();
    js_runtime_install_global_url();
    js_runtime_install_global_text();
    js_runtime_install_global_websocket();
    js_runtime_install_global_webcrypto();
    js_runtime_install_global_webfetch();
    js_runtime_install_proc_ipc();
    js_runtime_install_mod_dgram();
    js_runtime_install_mod_http2_constants();
    js_runtime_install_mod_node_test();
    // The user-facing diagnostics are always compiled; unit tests that reach
    // them expect the real builders. A shipped archive's installer does not
    // name them (the core archive would link the serializers).
    #[cfg(test)]
    js_runtime_install_diagnostics();
    // The always-compiled hosts (Node-API, WebAssembly) are not Cargo
    // features: perry installs them by name for the programs that need them,
    // so installing "everything compiled" (the core archive's installer) must
    // not pull them in.
}

/// Exact-deadline waits and native IO, selected by the compiler or an owning
/// module installer. Empty operation slots leave a program with no IO inert.
#[no_mangle]
pub extern "C" fn js_runtime_install_turnloop() {
    #[cfg(not(target_arch = "wasm32"))]
    perry_install_once!({
        crate::event_pump::loop_hooks::install();
    });
}

/// Root scanners an optional feature contributes. The root-scanner registry
/// is per thread: `gc_init` registers every filled slot on each thread it
/// initializes, and the install registers its scanner on the installing
/// thread (whose `gc_init` already ran).
/// The user-facing diagnostics (`v8.getHeapSnapshot` / `writeHeapSnapshot`,
/// `Bun.generateHeapSnapshot`, `process.report`): their JSON builders are
/// reached only through these slots, which `js_runtime_install_diagnostics`
/// fills, so a program that uses none of them links none of the serializers.
pub(crate) static JSON_FORMAT_ARG: Hook<unsafe fn(f64) -> String> = Hook::empty();

pub(crate) static HEAP_SNAPSHOT_JSON: Hook<fn(crate::gc::HeapSnapshotFormat) -> String> =
    Hook::empty();
pub(crate) static PROCESS_REPORT_JSON: Hook<fn(&str, Option<&str>) -> String> = Hook::empty();

pub(crate) static NODE_API_ROOT_SCANNER: Hook<crate::gc::MutableRootScanner> = Hook::empty();
pub(crate) static WASM_ROOT_SCANNER: Hook<crate::gc::MutableRootScanner> = Hook::empty();
pub(crate) static DGRAM_ROOT_SCANNER: Hook<crate::gc::MutableRootScanner> = Hook::empty();
pub(crate) static NODE_TEST_ROOT_SCANNER: Hook<crate::gc::MutableRootScanner> = Hook::empty();

/// Node-API addon hosting (#8523): `process.dlopen`, `require` of an
/// allowlisted `.node` addon, and the addon environment's pump, roots and
/// shutdown. perry installs it for a program with an approved native addon.
#[no_mangle]
pub extern "C" fn js_runtime_install_node_api_host() {
    js_runtime_install_bigint();
    js_runtime_install_turnloop();
    js_runtime_install_os();
    perry_install_once!({
        crate::node_api_host::install();
    });
}

/// The WebAssembly engine (#76). Installs only when a `perry-wasm-host`
/// archive registered its entry points (`js_runtime_set_wasm_host_ops`), which
/// perry links for a program that uses WebAssembly; otherwise the namespace
/// keeps its graceful-failure behavior.
#[no_mangle]
pub extern "C" fn js_runtime_install_wasm_host() {
    crate::wasm_hook_slots::install();
    js_runtime_install_bigint();
    perry_install_once!({
        crate::webassembly::install();
    });
}

/// The heap-snapshot and `process.report` builders. Always compiled: they
/// are user APIs, not instruments, so every archive carries them and perry
/// installs them for a program that references one.
#[no_mangle]
pub extern "C" fn js_runtime_install_diagnostics() {
    perry_install_once!({
        HEAP_SNAPSHOT_JSON.set(crate::gc::gc_build_v8_heap_snapshot_json_with_format);
        PROCESS_REPORT_JSON.set(crate::process::report::process_report_json_string);
        crate::diagnostic_report_roots::install();
    });
}

#[cfg(feature = "dyn-eval")]
#[no_mangle]
pub extern "C" fn js_runtime_install_dyn_eval() {
    perry_install_once!({
        crate::object::install_dyn_eval();
        crate::dyn_eval_hooks::install();
    });
}

#[cfg(feature = "bun-cli-utils")]
#[no_mangle]
pub extern "C" fn js_runtime_install_bun_cli_utils() {
    js_runtime_install_bigint();
    perry_install_once!({
        crate::bun_compat::install_cli_utils();
        crate::bun_hook_slots::install();
    });
}

#[cfg(feature = "intl-namespace")]
#[no_mangle]
pub extern "C" fn js_runtime_install_intl_namespace() {
    perry_install_once!({
        crate::intl::install_intl_namespace_feature();
    });
}

#[cfg(feature = "temporal")]
#[no_mangle]
pub extern "C" fn js_runtime_install_temporal() {
    js_runtime_install_bigint();
    perry_install_once!({
        crate::temporal::hooked::install();
    });
}

#[cfg(feature = "intl-datetime")]
#[no_mangle]
pub extern "C" fn js_runtime_install_intl_datetime() {
    perry_install_once!({
        crate::date::install_compiled_tzdb();
    });
}

#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_runtime_install_regex_engine() {
    perry_install_once!({
        crate::regex::install_iterator_hooks();
    });
}

/// URL value operations are available in every archive. Native module
/// installers use them even when the optional host canonicalizer is absent.
pub(crate) fn install_url_values() {
    perry_install_once!({
        crate::url_hooks::install_values();
    });
}

#[cfg(feature = "url-engine")]
#[no_mangle]
pub extern "C" fn js_runtime_install_url_engine() {
    perry_install_once!({
        crate::url::install_engine();
    });
    install_url_values();
}

#[no_mangle]
pub extern "C" fn js_runtime_install_string_normalize() {
    perry_install_once!({
        crate::string::normalize_hooked::install();
    });
}

#[no_mangle]
pub extern "C" fn js_runtime_install_intl_segmenter() {
    perry_install_once!({
        crate::intl::install_intl_segmenter_feature();
        crate::segmenter_hook_roots::install();
    });
}

#[no_mangle]
pub extern "C" fn js_runtime_install_intl_locale() {
    perry_install_once!({
        crate::intl::install_intl_locale_feature();
    });
}

#[no_mangle]
pub extern "C" fn js_runtime_install_global_math() {
    perry_install_once!({
        crate::object::global_this::web_hooks::install_math();
    });
}
#[no_mangle]
pub extern "C" fn js_runtime_install_global_json() {
    install_json_engine();
    crate::object::global_this::web_hooks::install_json();
}

#[no_mangle]
pub extern "C" fn js_runtime_install_global_reflect() {
    perry_install_once!({
        crate::object::global_this::web_hooks::install_reflect();
    });
}

#[no_mangle]
pub extern "C" fn js_runtime_install_global_atomics() {
    perry_install_once!({
        crate::object::global_this::web_hooks::install_atomics();
    });
}

#[no_mangle]
pub extern "C" fn js_runtime_install_global_url() {
    perry_install_once!({
        crate::object::global_this::web_hooks::install_url();
    });
}

#[no_mangle]
pub extern "C" fn js_runtime_install_global_text() {
    perry_install_once!({
        crate::object::global_this::web_hooks::install_text();
    });
}

#[no_mangle]
pub extern "C" fn js_runtime_install_global_websocket() {
    perry_install_once!({
        crate::object::global_this::web_hooks::install_websocket();
    });
}

#[no_mangle]
pub extern "C" fn js_runtime_install_global_webcrypto() {
    perry_install_once!({
        crate::object::global_this::web_hooks::install_webcrypto();
    });
}

#[no_mangle]
pub extern "C" fn js_runtime_install_global_webfetch() {
    install_json_engine();
    perry_install_once!({
        crate::object::global_this::web_hooks::install_webfetch();
    });
}

#[no_mangle]
pub extern "C" fn js_runtime_install_proc_ipc() {
    js_runtime_install_bigint();
    perry_install_once!({
        js_runtime_install_os();
        crate::process::ipc::install();
    });
}

#[no_mangle]
pub extern "C" fn js_runtime_install_mod_dgram() {
    perry_install_once!({
        crate::module_hooks::install_dgram();
    });
}

#[no_mangle]
pub extern "C" fn js_runtime_install_mod_http2_constants() {
    perry_install_once!({
        crate::module_hooks::install_http2_constants();
    });
}

#[no_mangle]
pub extern "C" fn js_runtime_install_mod_node_test() {
    perry_install_once!({
        crate::node_submodules::install_node_test();
        crate::node_test_hook_slots::install();
    });
}

#[cfg(test)]
mod diagnostics_install_tests {
    /// The heap-snapshot and `process.report` builders are user APIs, not
    /// instruments: after the install they answer in every build, with or
    /// without the `perry_diagnostics` instrument cfg.
    #[test]
    fn the_diagnostics_install_fills_the_user_api_builders() {
        super::js_runtime_install_diagnostics();
        assert!(super::PROCESS_REPORT_JSON.peek().is_some());
        let json = crate::node_v8::heap_snapshot_json();
        assert!(
            json.starts_with("{\"snapshot\""),
            "{}",
            &json[..json.len().min(80)]
        );
    }
}

/// Install the integer operations used by generic value hubs.
#[no_mangle]
pub extern "C" fn js_runtime_install_bigint() {
    perry_install_once!({
        crate::bigint_hooks::install();
    });
}

/// Format an already produced BigInt from a cross-crate generic value hub.
pub fn bigint_to_string(value: *const crate::bigint::BigIntHeader) -> *mut crate::StringHeader {
    crate::bigint_hooks::js_bigint_to_string(value)
}

#[no_mangle]
pub extern "C" fn js_runtime_install_proxy() {
    crate::perry_install_once!({
        crate::proxy_hooks::install();
    });
}
/// Formatting can stringify without publishing a JSON namespace.
#[no_mangle]
pub extern "C" fn js_runtime_install_json_format() {
    // The sole store is idempotent; no initialization work needs a Once.
    crate::builtins::install_json_format();
}

/// JSON value operations required by native producers (IPC, fetch, module
/// data), independently of exposing the global namespace member table.
pub(crate) fn install_json_engine() {
    crate::json_hooks::register_roots();
    crate::perry_install_once!({
        crate::builtins::install_json_format();
        crate::json_hooks::install();
    });
}

/// Direct JSON operations require the engine without a namespace object.
#[no_mangle]
pub extern "C" fn js_runtime_install_json_engine() {
    install_json_engine();
}

#[no_mangle]
pub extern "C" fn js_runtime_install_async_hooks() {
    crate::async_hook_slots::install();
}

pub(crate) static FS_REGISTER_ROOTS: Hook<fn()> = Hook::empty();

#[no_mangle]
pub extern "C" fn js_runtime_install_thread() {
    crate::thread_hook_slots::install();
}

#[no_mangle]
pub extern "C" fn js_runtime_install_date() {
    crate::date_hook_slots::install();
}

#[no_mangle]
pub extern "C" fn js_runtime_install_perf_hooks() {
    crate::perf_hook_slots::install();
}

#[no_mangle]
pub extern "C" fn js_runtime_install_os() {
    crate::os_hook_slots::install();
}

#[no_mangle]
pub extern "C" fn js_runtime_install_messaging() {
    crate::messaging_hook_slots::install();
}

#[no_mangle]
pub extern "C" fn js_runtime_install_event_target() {
    crate::event_target_hook_slots::install();
}

#[no_mangle]
pub extern "C" fn js_runtime_install_typedarray() {
    crate::typedarray_hook_slots::install();
}
