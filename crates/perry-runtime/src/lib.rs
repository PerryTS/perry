//! Runtime Library for Perry
//!
//! Provides the runtime support needed by compiled TypeScript programs:
//! - JSValue representation (NaN-boxing)
//! - Object representation and allocation
//! - Array representation and operations
//! - Garbage collection integration
//! - Built-in object implementations
//! - Console and other global functions

#![recursion_limit = "256"]

/// Issue #62: route every Rust heap allocation through mimalloc instead of
/// the system `malloc`. `gc_malloc`, arena block allocation, Vec/HashMap
/// growth inside the runtime, and the compiled-program side of the FFI all
/// use `std::alloc::{alloc, realloc, dealloc}`, which dispatch through the
/// global allocator — so flipping it here affects the entire hot path
/// (strings, closures, bigints, promises, object/array backing stores)
/// without touching any call sites. Per-thread segregated free lists cut
/// allocation dispatch from ~25-40ns (macOS `malloc`) to ~5-10ns, which is
/// meaningful because `gc_malloc` is called ~1M+ times/sec in allocation-
/// heavy workloads (string concat loops, JSON roundtrip, gc_pressure).
// arm64_32: mimalloc on a 32-bit-pointer (ILP32) tier-3 target is unproven and
// a corruption suspect; use the system allocator (libsystem_malloc, solid on
// watchOS) on 32-bit. Keep mimalloc's speed on 64-bit.
#[cfg(target_pointer_width = "64")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
#[cfg(not(target_pointer_width = "64"))]
#[global_allocator]
static GLOBAL: std::alloc::System = std::alloc::System;

pub mod abi_trampoline;
pub mod agent;
#[cfg(test)]
mod agent_dispatch_tests;
pub mod app_group;
pub mod arena;
pub mod array;
pub mod async_context;
pub mod async_hooks;
pub mod atomics;
pub mod atomics_futex;
pub mod bigint;
pub mod r#box;
pub mod buffer;
pub mod builtins;
pub mod bun_compat;
pub mod bun_ffi;
pub mod child_process;
pub mod closure;
pub mod cluster;
pub mod cluster_sched;
pub mod collection_iter;
pub mod collection_iter_object;
pub mod color_parse;
pub mod date;
#[cfg(feature = "mod-dgram")]
pub mod dgram;
#[cfg(feature = "mod-dgram")]
pub mod dgram_reactor;
pub mod disposable;
pub mod dns;
pub mod dns_resolver;
pub mod embedded;
pub mod error;
pub mod event_pump;
pub mod event_target;
pub mod exception;
pub mod fast_hash;
pub mod ffi;
pub mod frame;
pub mod fs;
pub mod gc;
pub mod intl;
pub mod iterator_helpers;
pub mod macos_bundle;
pub mod map;
pub mod math;
pub mod messaging;
pub mod module_require;
pub mod native_abi;
pub mod native_arena;
pub mod native_handle;
pub mod navigator;
pub mod net_validate;
// #6468: the `node:http2` constant tables are only reachable through the
// `http2` native-module namespace, so a program that never imports `node:http2`
// links none of them. The auto-optimizer enables `mod-http2-constants` on an
// `http2` import; every callsite has a benign `None`/no-op fallback when off.
#[cfg(feature = "mod-http2-constants")]
pub mod node_http2_constants;
pub mod node_inspector;
pub mod node_repl;
pub mod node_sea;
pub mod node_stream;
pub mod node_submodules;
pub mod node_test;
pub mod yoga;
// #3137/#3138/#3142: public `node:v8` serialize/deserialize + heap stats + GCProfiler.
pub mod node_v8;
// #3127/#3128/#3130/#3283: public `node:vm` import/require and narrowed execution.
pub mod node_vm;
// #6559: scoped tree-walking interpreter behind `new Function(p1, …, body)`
// with a runtime-constructed body (ajv / fast-json-stringify / find-my-way
// codegen). Parses via perry-parser (SWC) and bridges into the real runtime
// value model in both directions.
#[cfg(feature = "dyn-eval")]
pub mod dyn_eval;
// #2935: surface the zlib option-level resolver at the crate root so
// perry-stdlib's bundled codecs (and the `perry-ext-zlib` extern) can reach it.
pub use node_submodules::{
    js_zlib_resolve_level, js_zlib_validate_buffer_arg, js_zlib_validate_options,
};
pub mod object;
pub mod os;
pub mod path;
pub mod perf_hooks;
pub mod pointer_event;
pub mod process;
pub mod promise;
pub mod pty;
pub mod punycode;
pub mod readline_helpers;
pub mod regex;
pub mod safe_area;
pub mod set;
pub mod shared_sab;
pub(crate) mod state;
pub mod string;
pub mod symbol;
/// TC39 Temporal API (#4686): `Temporal.Duration`, `Temporal.Instant`,
/// `Temporal.PlainDate`, … wrapping the pure-Rust `temporal_rs` engine.
pub mod temporal;
pub mod text;
pub mod timer;
pub mod typed_feedback;
pub mod typedarray;
pub mod typedarray_half;
pub(crate) mod typedarray_props;
pub mod typedarray_view;
pub mod url;
pub mod v8;
pub mod validators;
pub mod value;
pub mod wasi;
pub mod web_storage;
/// WebAssembly host shims (issue #76). Forward-declares the
/// `perry_wasm_host_*` C ABI; the wasmi-backed implementation lives in
/// the separate `perry-wasm-host` crate and is linked in only when the
/// user passes `--enable-wasm-runtime`.
///
/// Gated behind the `wasm-host` Cargo feature so non-wasm programs don't
/// pull `js_webassembly_*` into libperry_runtime.a — those shims hold
/// undefined references to `perry_wasm_host_*` which would fail to link
/// without libperry_wasm_host.a on the line. The auto-optimize path
/// (crates/perry/src/commands/compile/optimized_libs.rs) flips this
/// feature on when `ctx.needs_wasm_runtime` is true.
#[cfg(feature = "wasm-host")]
pub mod webassembly;
// `net` moved to `perry-stdlib::net` (event-driven async) in A1/A1.5.
// The old sync `perry-runtime::net` module is retained as source but
// not exported so its `js_net_socket_{write,end,destroy}` symbols don't
// collide with the new stdlib ones. Delete the file entirely once no
// in-tree code references it.
// pub mod net;
#[cfg(feature = "ohos-napi")]
pub mod arkts_callbacks;
pub mod geisterhand_registry;
pub mod i18n;
#[cfg(all(
    any(target_os = "ios", target_os = "tvos", target_os = "visionos"),
    feature = "ios-game-loop"
))]
pub mod ios_game_loop;
pub mod json;
pub mod json_tape;
pub mod jsx;
/// HarmonyOS streaming media playback (`perry/media`) — drain-queue
/// bridge to `@ohos.multimedia.media.AVPlayer`. Symbols mirror the per-
/// platform `media_playback.rs` modules in the perry-ui-* crates; on
/// harmonyos there's no perry-ui-harmonyos so they live here. Issue #369.
#[cfg(feature = "ohos-napi")]
pub mod media_playback;
#[cfg(feature = "ohos-napi")]
pub mod ohos_napi;
#[cfg(feature = "full")]
pub mod plugin;
pub mod proxy;
pub mod static_plugins;
#[cfg(not(feature = "stdlib"))]
pub mod stdlib_stubs;
/// First-call runtime diagnostic for no-op FFI stubs (#464). Owns the
/// `PERRY_STUB_DIAG` env-var policy and the auto-generated `STUB_MANIFEST`
/// derived from `build.rs`'s dispatch-table walk.
pub mod stub_diag;
pub mod thread;
pub mod tls;
/// TTY support (#347 Phase 3): tty.isatty, process.std{in,out,err}.isTTY,
/// process.stdout.columns/.rows, SIGWINCH 'resize' event handler. Lives
/// in runtime (not stdlib) because it's a thin libc wrapper with no
/// async-runtime dependency. The pump drain is hooked separately.
pub mod tty;
/// Native TUI engine (#358): cell-grid + double-buffered renderer +
/// widget tree + FFI surface for `import { Box, Text, render } from
/// "perry/tui"`. Lives in runtime so the `js_perry_tui_*` symbols are
/// always available; no separate cargo dep, no async-runtime, no
/// per-program link flag.
pub mod tui;
/// HarmonyOS perry/ui FFI no-op stubs (#395). Auto-generated by
/// build.rs from perry-dispatch tables. Compiled only when `ohos-napi`
/// is on so the platform UI crates' definitions own the symbols on
/// every other target. See module docs for the harvest-model story.
#[cfg(feature = "ohos-napi")]
mod ui_harmonyos_stubs;
/// Cross-platform showToast / setText handler registry (Phase 2 v3.3).
/// Always compiled — provides `perry_arkts_*` stubs on non-harmonyos
/// builds so the codegen at lower_call/native.rs links cleanly without
/// target-aware branching. UI crates register their handlers here at
/// startup. See module docs for the ohos-napi gating story.
pub mod ui_text_registry;
pub mod util_abort;
pub mod util_call_sites;
pub mod util_debuglog;
pub mod util_diff;
pub mod util_inherits;
pub mod util_mime;
pub mod util_parse_args;
pub mod util_parse_env;
pub mod util_promisify;
pub mod util_settracesigint;
pub mod util_style_text;
pub mod util_syserr;
pub mod util_usv;
#[cfg(all(target_os = "watchos", feature = "watchos-game-loop"))]
pub mod watchos_game_loop;
pub mod weakref;
/// Shared Win32 console helpers: GetStdHandle/GetConsoleMode plumbing,
/// raw-input mode save/restore, window size, and the startup
/// ENABLE_VIRTUAL_TERMINAL_PROCESSING hook. Used by `tty` (isatty,
/// columns/rows, setRawMode), `tui::input` (render-loop raw mode) and
/// `gc::js_gc_init` (VT output at program start).
#[cfg(windows)]
pub mod win_console;

pub use array::ArrayHeader;
pub use bigint::BigIntHeader;
pub use buffer::BufferHeader;
pub use closure::ClosureHeader;
pub use map::MapHeader;
pub use object::ObjectHeader;
pub use promise::Promise;
pub use regex::RegExpHeader;
pub use set::SetHeader;
pub use string::StringHeader;
pub use value::JSValue;

// Re-export closure module for stdlib to use js_closure_call* functions
pub use closure::{js_closure_call0, js_closure_call1, js_closure_call2, js_closure_call3};

// Re-export commonly used FFI functions for stdlib
pub use array::js_array_push_f64;
pub use array::{
    js_array_alloc, js_array_get, js_array_get_jsvalue, js_array_is_array, js_array_length,
    js_array_push, js_array_set,
};
pub use bigint::js_bigint_from_string;
pub use object::js_object_set_field_by_name;
pub use object::{
    js_object_alloc, js_object_alloc_null_proto, js_object_alloc_with_shape, js_object_entries,
    js_object_get_field, js_object_get_field_by_name, js_object_get_field_by_name_f64,
    js_object_get_own_field_or_undef, js_object_get_unboxed_f64_field, js_object_keys,
    js_object_set_field, js_object_set_field_f64, js_object_set_keys,
    js_object_set_unboxed_f64_field, js_object_values,
};
pub use promise::{js_is_promise, js_promise_run_microtasks, js_promise_state, js_promise_value};
pub use promise::{
    js_promise_mark_internally_handled, js_promise_new, js_promise_reject, js_promise_rejected,
    js_promise_resolve, js_promise_resolved,
};
pub use string::js_string_from_bytes;
pub use value::{
    js_get_string_pointer_unified, js_jsvalue_to_string, js_nanbox_get_pointer, js_nanbox_pointer,
    js_nanbox_string,
};
pub use value::{
    js_set_handle_array_get, js_set_handle_array_length, js_set_handle_call_method,
    js_set_handle_object_get_property, js_set_handle_to_string, js_set_handle_typeof,
    js_set_native_async_hooks_construct, js_set_native_crypto_dispatch,
    js_set_native_domain_dispatch, js_set_native_events_construct, js_set_native_http_dispatch,
    js_set_native_module_js_loader, js_set_native_querystring_dispatch,
    js_set_native_sqlite_dispatch, js_set_native_tls_dispatch, js_set_native_webcrypto_dispatch,
    js_set_native_zlib_dispatch, js_set_new_from_handle_v8,
};

// Extension pump registration — allows extensions to register pump functions
// that run on each timer tick without hard-link dependencies.
mod ext_pump {
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicPtr, Ordering};

    static EXT_PUMP_FN: AtomicPtr<()> = AtomicPtr::new(null_mut());

    /// Register an extension's process_pending function pointer.
    /// Called by extensions during initialization.
    #[no_mangle]
    pub extern "C" fn js_register_ext_pump(f: extern "C" fn() -> i32) {
        EXT_PUMP_FN.store(f as *mut (), Ordering::Release);
    }

    /// Run the registered extension pump if available. Safe to call even if no
    /// extension is linked (no-op in that case).
    #[no_mangle]
    pub extern "C" fn js_run_ext_pump() {
        let f = EXT_PUMP_FN.load(Ordering::Acquire);
        if !f.is_null() {
            unsafe {
                let func: extern "C" fn() -> i32 = std::mem::transmute(f);
                func();
            }
        }
    }
}

// Stdlib pump registration — allows perry-ui-macos pump timer to call
// js_stdlib_process_pending without a hard link dependency on perry-stdlib.
mod stdlib_pump {
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicPtr, Ordering};
    use std::sync::Mutex;

    static STDLIB_PUMP_FN: AtomicPtr<()> = AtomicPtr::new(null_mut());

    // #2532 — auxiliary pump / has-active registries.
    //
    // perry-stdlib owns the single `STDLIB_PUMP_FN` slot above and drains
    // every in-tree module's pending queue from there. But the
    // `perry-ext-*` wrapper crates (perry-ext-http-server's request queue,
    // perry-ext-http's client response queue, …) are normally drained by
    // `js_stdlib_process_pending`'s `#[cfg(feature = "external-*-pump")]`
    // arms — which are only compiled in when the *workspace* auto-optimize
    // rebuilds perry-stdlib with that feature.
    //
    // In an out-of-tree install there is no workspace to rebuild from, so
    // the link uses the prebuilt full `libperry_stdlib.a` with those pump
    // arms compiled OUT. The ext lib would then link but its queue would
    // never be drained — a `node:http` server accepts connections that
    // nobody dispatches and the program hangs.
    //
    // These registries let each linked ext crate register its own
    // `*_process_pending` / `*_has_active` directly with the runtime, which
    // drains them on every tick regardless of which perry-stdlib features
    // are present. Registration is idempotent (a given fn pointer is only
    // stored once), so the in-tree path's compile-time arm calling the same
    // function is harmless — the second drain of an already-empty queue is
    // a no-op.
    static AUX_PUMPS: Mutex<Vec<extern "C" fn() -> i32>> = Mutex::new(Vec::new());
    static AUX_HAS_ACTIVE: Mutex<Vec<extern "C" fn() -> i32>> = Mutex::new(Vec::new());

    /// Register an auxiliary pump callback (a `perry-ext-*` crate's
    /// `*_process_pending`). Idempotent — registering the same function
    /// twice stores it once. Called the first time the ext crate's entry
    /// point runs (e.g. `js_node_http_create_server`).
    #[no_mangle]
    pub extern "C" fn js_register_aux_pump(f: extern "C" fn() -> i32) {
        if let Ok(mut pumps) = AUX_PUMPS.lock() {
            if !pumps.contains(&f) {
                pumps.push(f);
            }
        }
    }

    /// Register an auxiliary has-active callback (a `perry-ext-*` crate's
    /// `*_has_active`). Idempotent. Keeps the event loop alive while the
    /// ext crate reports live handles (e.g. a listening HTTP server).
    #[no_mangle]
    pub extern "C" fn js_register_aux_has_active(f: extern "C" fn() -> i32) {
        if let Ok(mut fns) = AUX_HAS_ACTIVE.lock() {
            if !fns.contains(&f) {
                fns.push(f);
            }
        }
    }

    /// Drain every registered auxiliary pump, returning the total work done.
    fn run_aux_pumps() -> i32 {
        let fns: Vec<extern "C" fn() -> i32> = match AUX_PUMPS.lock() {
            Ok(g) => g.clone(),
            Err(_) => return 0,
        };
        let mut count = 0i32;
        for f in fns {
            count = count.saturating_add(f());
        }
        count
    }

    /// True if any registered auxiliary has-active callback reports live work.
    fn aux_has_active() -> bool {
        let fns: Vec<extern "C" fn() -> i32> = match AUX_HAS_ACTIVE.lock() {
            Ok(g) => g.clone(),
            Err(_) => return false,
        };
        fns.iter().any(|f| f() != 0)
    }

    /// Register the stdlib's process_pending function pointer.
    /// Called by perry-stdlib during initialization.
    #[no_mangle]
    pub extern "C" fn js_register_stdlib_pump(f: extern "C" fn() -> i32) {
        STDLIB_PUMP_FN.store(f as *mut (), Ordering::Release);
    }

    /// Run the registered stdlib pump if available. Safe to call even if perry-stdlib
    /// is not linked (no-op in that case).
    #[no_mangle]
    pub extern "C" fn js_run_stdlib_pump() {
        crate::promise::js_native_async_process_pending();
        crate::os::js_process_signal_drain();
        // Drain the tty resize-pending flag (#347 Phase 3). Lives in
        // perry-runtime, not stdlib, so it runs even when stdlib isn't
        // linked — a TUI program that uses process.stdout.on('resize')
        // without importing any stdlib module still sees its callback
        // fire on SIGWINCH.
        crate::tty::js_tty_resize_drain();
        // #1934: drive the child_process spawn reactor — emit pending
        // spawn/data/end/exit/close for live children. Lives in perry-runtime,
        // so it runs even when perry-stdlib isn't linked. Zero-cost (one relaxed
        // atomic load) when there are no live children.
        crate::child_process::reactor::cp_reactor_pump();
        // #6563: drive the node-pty reactor — deliver pending onData/onExit
        // for live ptys. Zero-cost (one relaxed load) when none are live.
        #[cfg(unix)]
        crate::pty::reactor::pty_reactor_pump();
        // #4911: deliver queued UDP datagrams as `'message'` events. Lives in
        // perry-runtime so node:dgram works without perry-stdlib linked.
        // Zero-cost (one relaxed load) when no sockets are bound.
        #[cfg(feature = "mod-dgram")]
        crate::dgram_reactor::pump();
        crate::process::js_process_ipc_drain();
        let f = STDLIB_PUMP_FN.load(Ordering::Acquire);
        if !f.is_null() {
            unsafe {
                let func: extern "C" fn() -> i32 = std::mem::transmute(f);
                func();
            }
        }
        // #2532 — drain any `perry-ext-*` pumps registered directly with
        // the runtime (out-of-tree installs where perry-stdlib's
        // compile-time `external-*-pump` arms aren't present).
        run_aux_pumps();
        let _ = crate::gc::gc_runtime_safepoint();
    }

    static STDLIB_HAS_ACTIVE_FN: AtomicPtr<()> = AtomicPtr::new(null_mut());

    /// Register the stdlib's has_active_handles function pointer.
    /// Called by perry-stdlib during initialization.
    #[no_mangle]
    pub extern "C" fn js_register_stdlib_has_active(f: extern "C" fn() -> i32) {
        STDLIB_HAS_ACTIVE_FN.store(f as *mut (), Ordering::Release);
    }

    /// Check if the stdlib has active event sources (WS servers, pending
    /// async ops, etc.). Returns 0 if perry-stdlib is not linked.
    #[no_mangle]
    pub extern "C" fn js_stdlib_has_active_handles() -> i32 {
        if crate::promise::js_native_async_has_active() != 0 {
            return 1;
        }
        // #1934: a live spawn-reactor child keeps the event loop alive even when
        // perry-stdlib isn't linked (or reports no handles).
        if crate::child_process::reactor::cp_reactor_has_live() {
            return 1;
        }
        // #6563: a live pty keeps the event loop alive (its onData/onExit
        // handlers are still pending), like a live spawn-reactor child.
        #[cfg(unix)]
        if crate::pty::reactor::pty_reactor_has_live() {
            return 1;
        }
        // #4911: a bound + `ref`'d node:dgram socket keeps the loop alive.
        #[cfg(feature = "mod-dgram")]
        if crate::dgram_reactor::has_active() {
            return 1;
        }
        if crate::process::js_process_ipc_has_active() != 0 {
            return 1;
        }
        if crate::os::js_process_signal_has_active() != 0 {
            return 1;
        }
        // #2532 — a live `perry-ext-*` handle (e.g. a listening HTTP
        // server registered out-of-tree) keeps the loop alive even when
        // perry-stdlib reports none.
        if aux_has_active() {
            return 1;
        }
        let f = STDLIB_HAS_ACTIVE_FN.load(Ordering::Acquire);
        if !f.is_null() {
            unsafe {
                let func: extern "C" fn() -> i32 = std::mem::transmute(f);
                func()
            }
        } else {
            0
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::atomic::{AtomicI32, Ordering as AtomicOrdering};

        static PUMP_CALLS: AtomicI32 = AtomicI32::new(0);
        extern "C" fn counting_pump() -> i32 {
            PUMP_CALLS.fetch_add(1, AtomicOrdering::SeqCst);
            0
        }

        #[test]
        fn aux_pump_registration_is_idempotent() {
            // Registering the same fn pointer repeatedly stores it once,
            // so the in-tree compile-time arm calling the same function
            // can't multiply the per-tick drain count.
            js_register_aux_pump(counting_pump);
            js_register_aux_pump(counting_pump);
            js_register_aux_pump(counting_pump);
            let before = PUMP_CALLS.load(AtomicOrdering::SeqCst);
            run_aux_pumps();
            let after = PUMP_CALLS.load(AtomicOrdering::SeqCst);
            assert_eq!(
                after - before,
                1,
                "counting_pump must be invoked exactly once per run_aux_pumps despite triple registration"
            );
        }
    }
}

// Module init guard for preventing circular dependency stack overflow.
// Uses a simple bitset in the runtime so the compiler cannot optimize it away.
mod init_guard {
    use std::sync::atomic::{AtomicU8, Ordering};

    // Support up to 2048 modules (256 bytes). Each bit = one module.
    const GUARD_BYTES: usize = 256;
    static INIT_GUARD: [AtomicU8; GUARD_BYTES] = {
        const ZERO: AtomicU8 = AtomicU8::new(0);
        [ZERO; GUARD_BYTES]
    };

    /// Check and set the init guard for a module. Returns 1 if already set (skip init),
    /// 0 if not set (proceed with init). The guard is set atomically.
    #[no_mangle]
    pub extern "C" fn perry_init_guard_check_and_set(module_id: u64) -> i32 {
        let byte_idx = (module_id as usize) / 8;
        let bit_idx = (module_id as usize) % 8;
        if byte_idx >= GUARD_BYTES {
            return 0; // Out of range, don't guard
        }
        let mask = 1u8 << bit_idx;
        let prev = INIT_GUARD[byte_idx].fetch_or(mask, Ordering::SeqCst);
        if prev & mask != 0 {
            1
        } else {
            0
        }
    }
}

/// Lightweight runtime init for widget extensions.
/// Sets up GC, arena, and string interning without starting tokio or the full async runtime.
/// Called from generated Swift/Kotlin glue before invoking the native provider function.
#[no_mangle]
pub extern "C" fn perry_runtime_widget_init() {
    gc::js_gc_init();

    // Install early panic hook so we capture panics that happen before App()
    std::panic::set_hook(Box::new(|info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        let location = if let Some(loc) = info.location() {
            format!(" at {}:{}", loc.file(), loc.line())
        } else {
            String::new()
        };
        let full = format!("PERRY PANIC: {}{}\n", msg, location);
        // Write to stderr (may not be visible on iOS)
        eprintln!("{}", full);
        // Write to a file in the app's Documents directory
        if let Ok(home) = std::env::var("HOME") {
            let path = format!("{}/Documents/perry-crash.log", home);
            let _ = std::fs::write(&path, full.as_bytes());
        }
        // Also try the tmp directory (always writable on iOS)
        let _ = std::fs::write("/tmp/perry-crash.log", full.as_bytes());
    }));
}
