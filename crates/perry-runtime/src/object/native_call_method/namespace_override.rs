//! #10848: a user replacement of a native namespace method (`console.error =
//! f`, `console[m] = f`) called through the dynamic dispatcher.
//!
//! Every READ of `console.error` already returns the replacement; the call
//! used to go straight to the native implementation and ignore it.

use crate::exception::CatchStack;

crate::perry_thread_local! {
    /// `(module, method)` pairs whose user override is executing on this
    /// thread. A JS throw crossing the override skips the pop below; the
    /// catching landing pad pops the entry by its frame mark
    /// (`crate::exception::marks`).
    static RUNNING: std::cell::RefCell<CatchStack<(String, String)>> =
        const { std::cell::RefCell::new(CatchStack::new()) };
}

pub(crate) fn namespace_override_unwind_to(sp: usize) {
    RUNNING.with(|r| {
        r.borrow_mut().unwind_to(sp);
    });
}

#[cfg(any(test, target_os = "wasi"))]
pub(crate) fn namespace_override_restore_depth(depth: usize) {
    RUNNING.with(|r| {
        r.borrow_mut().restore_depth(depth);
    });
}

pub(crate) fn namespace_override_check_marks(sp: usize) {
    RUNNING.with(|r| {
        crate::exception::marks::check_marks(r.borrow().marks(), sp, "namespace_override")
    });
}

#[cfg(any(test, target_os = "wasi"))]
pub(crate) fn namespace_override_depth() -> usize {
    RUNNING.with(|r| r.borrow().len())
}

#[cfg(test)]
pub(crate) fn test_push_catch_namespace_override(marker: u32, mark: usize) {
    RUNNING.with(|r| {
        r.borrow_mut()
            .push(mark, ("catch-savepoint".to_string(), marker.to_string()))
    });
}

/// Invoke a user replacement of `module.method_name` with the namespace as
/// `this`. `None` when the method was not replaced (or holds a non-callable),
/// so the caller dispatches to the native implementation.
///
/// The original builtin value — a bound-native closure that re-enters the
/// dispatcher for the same `(module, method)` — is a common replacement: the
/// restore half of a capture (`const orig = console.log; …; console.log =
/// orig`), or a wrapper that forwards to it (`console.log = orig.bind(console)`).
/// While an override for a pair is running, a nested dispatch of the same pair
/// therefore goes native instead of finding the override again and recursing
/// until the depth guard returns a stub (which printed nothing).
pub(super) unsafe fn call_native_namespace_override(
    root_scope: &crate::gc::RuntimeHandleScope,
    namespace: f64,
    module: String,
    method_name: &str,
    args: &[f64],
) -> Option<f64> {
    let running = RUNNING.with(|r| {
        r.borrow()
            .iter()
            .any(|(m, n)| *m == module && n == method_name)
    });
    if running {
        return None;
    }
    let namespace_handle = root_scope.root_nanbox_f64(namespace);
    let args_handles = root_scope.root_nanbox_f64_slice(args);
    // Allocates (the probe key string): everything live is rooted above.
    let own = crate::object::native_module::native_namespace_user_value(&module, method_name)?;
    if !crate::closure::is_closure_ptr(crate::value::js_nanbox_get_pointer(own) as usize) {
        return None;
    }
    let method_handle = root_scope.root_nanbox_f64(own);
    let bound = crate::closure::clone_closure_rebind_this(
        method_handle.get_nanbox_f64().to_bits(),
        namespace_handle.get_nanbox_f64(),
    );
    let bound_handle = root_scope.root_nanbox_f64(f64::from_bits(bound));

    struct Running;
    impl Drop for Running {
        fn drop(&mut self) {
            RUNNING.with(|r| {
                r.borrow_mut().pop();
            });
        }
    }
    let mark = crate::exception::frame_mark();
    RUNNING.with(|r| r.borrow_mut().push(mark, (module, method_name.to_string())));
    let _running = Running;
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&args_handles);
    Some(crate::closure::native_call_value_this(
        bound_handle.get_nanbox_f64(),
        crate::closure::JsThis::from_f64(namespace_handle.get_nanbox_f64()),
        args.as_ptr(),
        args.len(),
    ))
}
