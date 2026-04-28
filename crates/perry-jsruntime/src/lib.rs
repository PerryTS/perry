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

pub use bridge::{native_to_v8, v8_to_native, store_js_handle, get_js_handle, release_js_handle,
    is_js_handle, get_handle_id, make_js_handle_value};
pub use interop::{
    js_call_function, js_call_method, js_get_export, js_load_module, js_register_native_function,
    js_runtime_init, js_runtime_shutdown, js_handle_object_get_property, js_set_property,
    js_new_instance, js_new_from_handle, js_create_callback,
    js_handle_array_get, js_handle_array_length,
};
// Re-export deno_core's ModuleLoader trait for external use
pub use deno_core::ModuleLoader;

// Re-export perry-stdlib to include all its symbols in this staticlib
pub use perry_stdlib;

use deno_core::{JsRuntime, RuntimeOptions};
use once_cell::sync::OnceCell;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
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
    /// Whether the runtime has been initialized
    pub initialized: bool,
}

impl JsRuntimeState {
    fn new() -> Self {
        let mut runtime = JsRuntime::new(RuntimeOptions {
            module_loader: Some(std::rc::Rc::new(modules::NodeModuleLoader::new())),
            extensions: vec![ops::perry_ops::init_ops()],
            ..Default::default()
        });

        // Set V8 stack limit based on actual thread stack bounds.
        // Previously set to 0x10000 which disabled V8's stack overflow detection entirely,
        // causing SIGBUS on arm64 when deep call chains (module init → async → V8 eval)
        // overflowed past the stack guard page.
        //
        // The Rust v8 bindings (v8 0.106) don't expose Isolate::SetStackLimit,
        // so we call the C++ function directly via its Itanium ABI mangled name.
        {
            extern "C" {
                #[link_name = "_ZN2v87Isolate13SetStackLimitEm"]
                fn v8_isolate_set_stack_limit(isolate: *mut std::ffi::c_void, stack_limit: usize);
            }
            let isolate: &mut deno_core::v8::Isolate = runtime.v8_isolate();
            let isolate_ptr: *mut std::ffi::c_void = (isolate as *mut deno_core::v8::Isolate).cast();

            // Compute stack limit from actual thread stack bounds
            #[cfg(target_os = "macos")]
            let stack_limit = {
                extern "C" {
                    fn pthread_self() -> *mut std::ffi::c_void;
                    fn pthread_get_stackaddr_np(thread: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
                    fn pthread_get_stacksize_np(thread: *mut std::ffi::c_void) -> usize;
                }
                let thread = unsafe { pthread_self() };
                let stack_addr = unsafe { pthread_get_stackaddr_np(thread) } as usize;
                let stack_size = unsafe { pthread_get_stacksize_np(thread) };
                let stack_bottom = stack_addr - stack_size;
                // Reserve 64KB above stack bottom as safety margin for V8's stack check
                stack_bottom + 64 * 1024
            };
            #[cfg(not(target_os = "macos"))]
            let stack_limit: usize = 0x10000;

            unsafe { v8_isolate_set_stack_limit(isolate_ptr, stack_limit); }
        }

        // Set up Node.js global polyfills before any modules are loaded
        runtime.execute_script("<node-polyfills>", deno_core::ascii_str_include!("node_polyfills.js"))
            .expect("Failed to initialize Node.js polyfills");

        Self {
            runtime,
            loaded_modules: HashMap::new(),
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

pub fn stash_trampoline_scope(scope: &mut deno_core::v8::HandleScope) -> TrampolineScopeGuard {
    let prev = REENTRY_SCOPE_PTR.with(|p| p.get());
    let scope_ptr = scope as *mut deno_core::v8::HandleScope as *mut std::ffi::c_void;
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
pub unsafe fn try_trampoline_scope<'a>() -> Option<&'a mut deno_core::v8::HandleScope<'a>> {
    let stashed = REENTRY_SCOPE_PTR.with(|p| p.get());
    if stashed.is_null() {
        return None;
    }
    // Cast back to a HandleScope reference. The lifetime 'a is unconstrained
    // here — it's the caller's responsibility to use the reference only
    // within the trampoline's frame lifetime.
    let scope: &mut deno_core::v8::HandleScope<'_> =
        &mut *(stashed as *mut deno_core::v8::HandleScope<'_>);
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
