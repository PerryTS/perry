//! V8 JavaScript Runtime for Perry
//!
//! This crate provides V8 JavaScript runtime support for running npm modules
//! that cannot be natively compiled. It serves as a fallback when:
//! - A module is pure JavaScript (not TypeScript)
//! - A module uses dynamic features incompatible with AOT compilation
//!
//! The runtime is opt-in and requires explicit configuration.

mod bridge;
mod interop;
mod modules;
mod ops;

pub use bridge::{
    get_handle_id, get_js_handle, is_js_handle, make_js_handle_value, native_to_v8,
    release_js_handle, store_js_handle, v8_to_native,
};
pub use interop::{
    js_call_function, js_call_method, js_create_callback, js_get_export, js_handle_array_get,
    js_handle_array_length, js_handle_object_get_property, js_load_module, js_new_from_handle,
    js_new_instance, js_register_native_function, js_runtime_init, js_runtime_shutdown,
    js_set_property,
};
// Re-export deno_core's ModuleLoader trait for external use
pub use deno_core::ModuleLoader;

// Re-export perry-stdlib to include all its symbols in this staticlib
pub use perry_stdlib;

use deno_core::{JsRuntime, RuntimeOptions};
use once_cell::sync::OnceCell;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use tokio::runtime::Runtime as TokioRuntime;

/// Global Tokio runtime for async operations
static TOKIO_RUNTIME: OnceCell<TokioRuntime> = OnceCell::new();

thread_local! {
    /// Thread-local V8 runtime instance
    /// JsRuntime is not Send, so it must be thread-local
    static JS_RUNTIME: RefCell<Option<JsRuntimeState>> = const { RefCell::new(None) };

    /// Issue #255 — re-entrancy escape hatch. While the outer `with_runtime`
    /// holds the `JS_RUNTIME.borrow_mut()` lock, V8 can call back into Perry
    /// (via `native_callback_trampoline` → Perry closure body), which may
    /// then call FFIs like `js_get_property` that themselves go through
    /// `with_runtime` again. Pre-fix the inner `borrow_mut()` panicked with
    /// "RefCell already borrowed". This raw-pointer mirror lets the inner
    /// call reuse the outer's `&mut JsRuntimeState` instead of trying to
    /// acquire a second borrow. Lifetime: the pointer is valid only while
    /// the outer `with_runtime` body is on the stack; a Drop guard clears
    /// it on normal return AND on panic-unwind.
    static REENTRY_PTR: Cell<*mut JsRuntimeState> = const { Cell::new(std::ptr::null_mut()) };

    /// Issue #255 — V8 scope passthrough for callback re-entrancy. When V8
    /// invokes `native_callback_trampoline`, it gives us a live
    /// `&mut HandleScope` on the call stack. Re-entrant FFIs (`js_get_property`,
    /// `js_set_property`, etc. called from inside the Perry callback) MUST
    /// use that scope rather than calling `state.runtime.handle_scope()` —
    /// the latter clashes with deno_core's internal scope tracking and
    /// V8 panics with "active scope can't be dropped" on the inner scope's
    /// Drop. The trampoline stashes its scope pointer here on entry and
    /// clears it on exit (via Drop guard); FFIs check this stash and use
    /// the trampoline's scope directly when non-null. The pointer's `'static`
    /// lifetime is a lie — it's only valid while the trampoline frame is
    /// on the stack — but that's exactly the window where re-entrant FFIs
    /// can be called.
    static REENTRY_SCOPE_PTR: Cell<*mut std::ffi::c_void> = const { Cell::new(std::ptr::null_mut()) };
}

/// State for the JS runtime
pub struct JsRuntimeState {
    pub runtime: JsRuntime,
    /// Map of loaded module paths to their V8 module IDs
    pub loaded_modules: HashMap<PathBuf, deno_core::ModuleId>,
    /// Module evaluation futures started by `js_load_module` and driven by
    /// Perry's jsruntime pump instead of a blocking V8 event loop.
    pub pending_module_evaluations: HashMap<deno_core::ModuleId, PendingModuleEvaluation>,
    /// Whether the runtime has been initialized
    pub initialized: bool,
}

pub struct PendingModuleEvaluation {
    pub canonical_path: PathBuf,
    pub future: Pin<Box<dyn Future<Output = Result<(), deno_core::error::CoreError>>>>,
}

impl JsRuntimeState {
    fn new() -> Self {
        let mut runtime = JsRuntime::new(RuntimeOptions {
            module_loader: Some(std::rc::Rc::new(modules::NodeModuleLoader::new())),
            extensions: vec![ops::perry_ops::init()],
            ..Default::default()
        });

        // Note: previously this block called `Isolate::SetStackLimit` via the
        // Itanium ABI mangled name to fix arm64 SIGBUS on deep call chains.
        // After the v8 0.106 → 147 bump, calling that exported symbol while
        // the isolate is not "entered" (no `Isolate::Scope` on the stack)
        // silently exits the process with code 0 — v8 147's stack-guard
        // internals appear to abort cleanly instead of crashing. The
        // upstream `deno_core::scope!` macro already pins the isolate
        // properly for each work scope and v8 147 picks a sane default
        // stack limit from the calling thread's stack, so this manual
        // override is no longer required.

        // Set up Node.js global polyfills before any modules are loaded
        runtime
            .execute_script(
                "<node-polyfills>",
                deno_core::ascii_str_include!("node_polyfills.js"),
            )
            .expect("Failed to initialize Node.js polyfills");

        Self {
            runtime,
            loaded_modules: HashMap::new(),
            pending_module_evaluations: HashMap::new(),
            initialized: true,
        }
    }
}

/// Initialize the Tokio runtime for async operations
pub fn get_tokio_runtime() -> &'static TokioRuntime {
    TOKIO_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime")
    })
}

/// Issue #255 — set the trampoline's V8 scope as the re-entrancy escape
/// hatch. Returns a guard that clears the stash on Drop (LIFO so nested
/// trampoline invocations restore the previous scope).
///
/// **Safety**: caller must guarantee that `scope` outlives every FFI call
/// that might check the stash. In practice this is always true: the
/// trampoline holds `&mut HandleScope` on its stack frame while invoking
/// the Perry callback, and re-entrant FFIs only fire while the callback
/// is running.
pub struct TrampolineScopeGuard {
    prev: *mut std::ffi::c_void,
}

impl Drop for TrampolineScopeGuard {
    fn drop(&mut self) {
        REENTRY_SCOPE_PTR.with(|p| p.set(self.prev));
    }
}

pub fn stash_trampoline_scope(scope: &mut deno_core::v8::PinScope<'_, '_>) -> TrampolineScopeGuard {
    let prev = REENTRY_SCOPE_PTR.with(|p| p.get());
    let scope_ptr = scope as *mut deno_core::v8::PinScope<'_, '_> as *mut std::ffi::c_void;
    REENTRY_SCOPE_PTR.with(|p| p.set(scope_ptr));
    TrampolineScopeGuard { prev }
}

/// Issue #255 — try to get the trampoline's stashed V8 scope for
/// re-entrant FFI calls. Returns `Some(&mut HandleScope)` when called
/// from inside a `native_callback_trampoline` invocation,
/// `None` otherwise.
///
/// **Safety**: the returned reference is only valid for the duration of
/// the current synchronous call chain (until the trampoline's stack
/// frame is unwound). Don't store it across `await` points or threads.
///
/// # Safety
///
/// Caller must ensure the returned reference doesn't outlive the
/// trampoline frame that stashed it.
pub unsafe fn try_trampoline_scope<'a>() -> Option<&'a mut deno_core::v8::PinScope<'a, 'a>> {
    let stashed = REENTRY_SCOPE_PTR.with(|p| p.get());
    if stashed.is_null() {
        return None;
    }
    // Cast back to a PinScope reference. The lifetime 'a is unconstrained
    // here — it's the caller's responsibility to use the reference only
    // within the trampoline's frame lifetime.
    let scope: &mut deno_core::v8::PinScope<'_, '_> =
        &mut *(stashed as *mut deno_core::v8::PinScope<'_, '_>);
    Some(std::mem::transmute(scope))
}

/// Initialize the JS runtime for the current thread
pub fn ensure_runtime_initialized() {
    // Issue #255 — short-circuit when re-entered from a V8 callback.
    // The outer `with_runtime` already holds the borrow + has stashed its
    // `&mut JsRuntimeState` in `REENTRY_PTR`; doing `borrow_mut` here
    // again would panic. Since the state must already be initialized
    // (we couldn't be inside `with_runtime` otherwise), there's nothing
    // to do.
    if REENTRY_PTR.with(|p| !p.get().is_null()) {
        return;
    }
    // `JsRuntime::new()` captures `tokio::runtime::Handle::try_current()`
    // for its async-op executor. Without entering Perry's shared tokio
    // runtime here, async ops that touch `tokio::net` / `tokio::spawn`
    // would later panic with "no reactor running" because the captured
    // handle would be empty / point at a defunct runtime. Mirror this
    // enter() in `with_runtime` below so every poll of the JS event loop
    // sees the same reactor context.
    let tokio_rt = get_tokio_runtime();
    let _enter = tokio_rt.enter();
    JS_RUNTIME.with(|cell| {
        let mut opt = cell.borrow_mut();
        if opt.is_none() {
            *opt = Some(JsRuntimeState::new());
        }
    });
}

/// Execute a closure with the JS runtime.
///
/// **Re-entrancy (issue #255):** safe to call from inside a V8 callback
/// invoked while another `with_runtime` body is on the stack. The inner
/// call detects the outer's stashed `REENTRY_PTR` and reuses the same
/// `&mut JsRuntimeState` instead of trying to acquire a second
/// `RefCell::borrow_mut`. This is the standard callback-driven
/// re-entrancy pattern: the outer `&mut` reference is paused (control
/// is in V8 → trampoline → Perry callback) while the inner reference
/// is active, so they never alias in time.
pub fn with_runtime<F, R>(f: F) -> R
where
    F: FnOnce(&mut JsRuntimeState) -> R,
{
    // Re-entrant fast path: outer with_runtime is still on the stack;
    // reuse its &mut via the stashed raw pointer instead of borrowing again.
    let stashed = REENTRY_PTR.with(|p| p.get());
    if !stashed.is_null() {
        // SAFETY: REENTRY_PTR is non-null only while the outer
        // `with_runtime` body holds the RefCell borrow AND its &mut is
        // suspended on the call stack. The outer reference can't be used
        // concurrently because control is here, not at the outer's site.
        // The Drop guard below clears the pointer on return / panic so
        // the next outer call sees null again.
        let state = unsafe { &mut *stashed };
        return f(state);
    }

    ensure_runtime_initialized();
    // Enter the shared tokio runtime context so V8 async ops touching
    // `tokio::net` / `tokio::spawn` (e.g. the V8-fallback http server
    // ops) can run inside a reactor. See `ensure_runtime_initialized`.
    let tokio_rt = get_tokio_runtime();
    let _enter = tokio_rt.enter();
    JS_RUNTIME.with(|cell| {
        let mut opt = cell.borrow_mut();
        let state = opt.as_mut().expect("Runtime should be initialized");
        let state_ptr: *mut JsRuntimeState = state;

        // Stash the pointer so re-entrant calls (V8 → Perry callback →
        // js_get_property → with_runtime) take the fast path above.
        REENTRY_PTR.with(|p| p.set(state_ptr));
        // Guard clears the pointer on normal return AND on panic-unwind.
        // Without this, a panic during `f` would leave a dangling pointer
        // that the next thread-local user would dereference.
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                REENTRY_PTR.with(|p| p.set(std::ptr::null_mut()));
            }
        }
        let _guard = Guard;

        f(state)
    })
}

/// Execute an async closure with the JS runtime.
///
/// Same re-entrancy semantics as `with_runtime` (issue #255).
pub fn with_runtime_async<F, Fut, R>(f: F) -> R
where
    F: FnOnce(&mut JsRuntimeState) -> Fut,
    Fut: std::future::Future<Output = R>,
{
    let tokio_rt = get_tokio_runtime();
    tokio_rt.block_on(async {
        // Re-entrant fast path mirrors with_runtime.
        let stashed = REENTRY_PTR.with(|p| p.get());
        if !stashed.is_null() {
            // SAFETY: same as with_runtime — REENTRY_PTR is non-null only
            // while the outer with_runtime/with_runtime_async body holds
            // the borrow.
            let state = unsafe { &mut *stashed };
            return tokio::task::block_in_place(|| {
                let local_rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create local Tokio runtime");
                local_rt.block_on(f(state))
            });
        }

        ensure_runtime_initialized();
        JS_RUNTIME.with(|cell| {
            let mut opt = cell.borrow_mut();
            let state = opt.as_mut().expect("Runtime should be initialized");
            let state_ptr: *mut JsRuntimeState = state;
            REENTRY_PTR.with(|p| p.set(state_ptr));
            struct Guard;
            impl Drop for Guard {
                fn drop(&mut self) {
                    REENTRY_PTR.with(|p| p.set(std::ptr::null_mut()));
                }
            }
            let _guard = Guard;
            // Use a dedicated current-thread Tokio runtime to avoid thread pool starvation deadlock.
            // The outer block_on holds a worker thread; using Handle::current().block_on() would
            // create a nested block_on on the same runtime, deadlocking if async JS operations
            // spawn Tokio tasks (e.g., ethers.js HTTP calls).
            tokio::task::block_in_place(|| {
                let local_rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create local Tokio runtime");
                local_rt.block_on(f(state))
            })
        })
    })
}

// No tests in this module — `interop::tests::test_runtime_init` covers single-init.
// A separate test that called `js_runtime_init()` here used to live in lib.rs, but on Linux
// it segfaulted under `cargo test -p perry-jsruntime --lib`: deno_core/V8 don't tolerate a
// second `JsRuntime::new()` in the same process across cargo's per-test worker threads
// (see #196). The double-init tolerance the old test claimed to verify is trivially provided
// by the `if opt.is_none()` guard in `ensure_runtime_initialized` above.
