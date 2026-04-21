//! Runtime Library for Perry
//!
//! Provides the runtime support needed by compiled TypeScript programs:
//! - JSValue representation (NaN-boxing)
//! - Object representation and allocation
//! - Array representation and operations
//! - Garbage collection integration
//! - Built-in object implementations
//! - Console and other global functions

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
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub mod value;
pub mod gc;
pub mod arena;
pub mod color_parse;
pub mod object;
pub mod array;
pub mod map;
pub mod set;
pub mod string;
pub mod bigint;
pub mod closure;
pub mod exception;
pub mod error;
pub mod symbol;
pub mod promise;
pub mod timer;
pub mod event_pump;
pub mod builtins;
pub mod r#box;
pub mod process;
pub mod fs;
pub mod path;
pub mod math;
pub mod date;
pub mod url;
pub mod regex;
pub mod os;
pub mod buffer;
pub mod typedarray;
pub mod text;
pub mod child_process;
// `net` moved to `perry-stdlib::net` (event-driven async) in A1/A1.5.
// The old sync `perry-runtime::net` module is retained as source but
// not exported so its `js_net_socket_{write,end,destroy}` symbols don't
// collide with the new stdlib ones. Delete the file entirely once no
// in-tree code references it.
// pub mod net;
pub mod json;
pub mod json_tape;
pub mod i18n;
pub mod weakref;
pub mod static_plugins;
#[cfg(not(feature = "stdlib"))]
pub mod stdlib_stubs;
#[cfg(feature = "full")]
pub mod plugin;
pub mod thread;
pub mod geisterhand_registry;
pub mod proxy;
#[cfg(all(any(target_os = "ios", target_os = "tvos"), feature = "ios-game-loop"))]
pub mod ios_game_loop;
#[cfg(all(target_os = "watchos", feature = "watchos-game-loop"))]
pub mod watchos_game_loop;
#[cfg(feature = "ohos-napi")]
pub mod ohos_napi;

pub use value::JSValue;
pub use promise::Promise;
pub use object::ObjectHeader;
pub use array::ArrayHeader;
pub use map::MapHeader;
pub use set::SetHeader;
pub use string::StringHeader;
pub use bigint::BigIntHeader;
pub use closure::ClosureHeader;
pub use regex::RegExpHeader;
pub use buffer::BufferHeader;

// Re-export closure module for stdlib to use js_closure_call* functions
pub use closure::{js_closure_call0, js_closure_call1, js_closure_call2, js_closure_call3};

// Re-export commonly used FFI functions for stdlib
pub use array::{js_array_alloc, js_array_set, js_array_get, js_array_push, js_array_length, js_array_is_array, js_array_get_jsvalue};
pub use object::{js_object_alloc, js_object_alloc_with_shape, js_object_set_field, js_object_set_field_f64, js_object_get_field, js_object_set_keys, js_object_keys, js_object_values, js_object_entries, js_object_get_field_by_name, js_object_get_field_by_name_f64};
pub use string::js_string_from_bytes;
pub use promise::{js_promise_new, js_promise_resolve, js_promise_reject};
pub use bigint::js_bigint_from_string;
pub use value::{js_nanbox_get_pointer, js_nanbox_pointer, js_nanbox_string, js_get_string_pointer_unified, js_jsvalue_to_string};
pub use value::{js_set_handle_array_get, js_set_handle_array_length, js_set_handle_object_get_property, js_set_handle_to_string, js_set_handle_call_method, js_set_native_module_js_loader, js_set_new_from_handle_v8};
pub use array::{js_array_push_f64};
pub use object::js_object_set_field_by_name;
pub use promise::{js_promise_run_microtasks, js_promise_state, js_is_promise, js_promise_value};

// Stdlib pump registration — allows perry-ui-macos pump timer to call
// js_stdlib_process_pending without a hard link dependency on perry-stdlib.
mod stdlib_pump {
    use std::sync::atomic::{AtomicPtr, Ordering};
    use std::ptr::null_mut;

    static STDLIB_PUMP_FN: AtomicPtr<()> = AtomicPtr::new(null_mut());

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
        let f = STDLIB_PUMP_FN.load(Ordering::Acquire);
        if !f.is_null() {
            unsafe {
                let func: extern "C" fn() -> i32 = std::mem::transmute(f);
                func();
            }
        }
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
        if prev & mask != 0 { 1 } else { 0 }
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
