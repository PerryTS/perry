//! `process.emitWarning(error)` must not reshape the error (#9202).
//!
//! node's `emitWarning` validates the positional `type`/`code` and then ignores
//! both for an Error warning: the default reporter titles the warning from the
//! error's own `name`, adds `[code]` / `detail` from the error's own fields, and
//! `warning` listeners receive that SAME object. The stderr line those fields
//! produce is covered byte-for-byte by the parity fixtures
//! (`node-suite/events/max-listeners/warning` and
//! `node-suite/process/methods/emit-warning-*-event`); what is pinned here is
//! the object the reporter reads them from.

use std::cell::RefCell;

use super::*;

/// `(name, code, detail, message, is_the_same_object)` for every warning the
/// registered listener received.
thread_local! {
    static CAPTURED: RefCell<Vec<(String, String, String, String, bool)>> =
        const { RefCell::new(Vec::new()) };
    /// The error the test passed to `emitWarning`, held rooted for the drain so
    /// its bits are a valid identity comparison inside the listener.
    static EXPECTED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

extern "C" fn capture_warning(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    warning: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let bits = warning.to_bits();
    let warning = scope.root_raw_mut_ptr(
        crate::value::js_nanbox_get_pointer(warning) as *mut crate::object::ObjectHeader
    );
    let field = |name: &str| object_string_field(&warning, name).unwrap_or_default();
    let same = EXPECTED.with(|expected| expected.get() != 0 && expected.get() == bits);
    CAPTURED.with(|captured| {
        captured.borrow_mut().push((
            field("name"),
            field("code"),
            field("detail"),
            field("message"),
            same,
        ))
    });
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[test]
fn emit_warning_error_keeps_its_own_name_message_and_identity() {
    crate::os::test_clear_process_event_listeners();
    CAPTURED.with(|captured| captured.borrow_mut().clear());
    EXPECTED.with(|expected| expected.set(0));

    let listener = js_closure_alloc(crate::fn_info!(capture_warning, 1; with_declared(1)), 0);
    let listener = crate::value::js_nanbox_pointer(listener as i64);
    let event = string_value("warning");
    let _ = crate::os::js_process_on(event.to_bits() as i64, listener.to_bits() as i64);

    let scope = crate::gc::RuntimeHandleScope::new();
    let message = crate::string::js_string_from_bytes(b"boom".as_ptr(), 4);
    let error = crate::error::js_error_new_with_message(message);
    set_error_string_prop(error, "name", "CustomName");
    set_error_string_prop(error, "code", "EC");
    set_error_string_prop(error, "detail", "ED");
    let error = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(error as i64));
    EXPECTED.with(|expected| expected.set(error.get_nanbox_u64()));

    // A positional `type`/`code` is validated and then ignored for an Error
    // warning, exactly as node's `emitWarning` does.
    js_process_emit_warning(
        error.get_nanbox_f64(),
        string_value("IgnoredType"),
        string_value("IgnoredCode"),
    );
    let _ = crate::promise::js_promise_run_microtasks();

    CAPTURED.with(|captured| {
        let captured = captured.borrow();
        assert_eq!(captured.len(), 1, "the 'warning' listener ran once");
        assert_eq!(
            captured[0],
            (
                "CustomName".to_string(),
                "EC".to_string(),
                "ED".to_string(),
                "boom".to_string(),
                true
            ),
            "an Error warning reaches listeners with its own name/code/detail/\
             message, and is the very object the caller passed"
        );
    });
    crate::os::test_clear_process_event_listeners();
    EXPECTED.with(|expected| expected.set(0));
}
