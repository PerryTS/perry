//! A failed spawn has no live reactor entry to deliver output-pipe EOF.
//! Finish those empty streams after the child error and before child close.
use super::*;

/// Schedule close only after delivering the error. A close timer armed at spawn
/// time can already be overdue before the error's setImmediate callback runs.
/// Keep fork's separate failure contract unchanged by using this for spawn only.
pub(super) extern "C" fn emit_error_then_close(closure: *const ClosureHeader) -> f64 {
    let unhandled = {
        let scope = crate::gc::RuntimeHandleScope::new();
        let cp = scope.root_nanbox_f64(cp_this(closure));
        let unhandled = emit_spawn_error(cp.get_nanbox_f64());
        // An unhandled error ends the process in Node before `close` fires.
        if unhandled.is_none() {
            let close =
                crate::closure::js_closure_alloc(reactor::cp_emit_spawn_close as *const u8, 1);
            crate::closure::js_closure_set_capture_ptr(
                close,
                0,
                cp.get_nanbox_f64().to_bits() as i64,
            );
            crate::timer::js_set_timeout_callback(close as i64, 1.0);
        }
        unhandled
    };
    throw_if_unhandled(unhandled);
    cp_undefined()
}

/// Emit the deferred spawn/fork failure `error` on the ChildProcess `cp`.
///
/// Returns the error when no `error` listener took it (#10730). Node's
/// EventEmitter throws an `error` event nobody listens for, so a failed
/// `spawn("/no/such/file")` without a handler dies with `Unhandled 'error'
/// event`. Perry used to drop it and carry on, which made a missing binary
/// look like a child that ran and exited.
pub(super) fn emit_spawn_error(cp: f64) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cp = scope.root_nanbox_f64(cp);
    let err = scope.root_nanbox_f64(cp_get_field(cp.get_nanbox_f64(), b"__cpError"));
    if JSValue::from_bits(err.get_nanbox_f64().to_bits()).is_undefined() {
        return None;
    }
    let handled = cp_emit(cp.get_nanbox_f64(), "error", &[err.get_nanbox_f64()]);
    cp_set_field(cp.get_nanbox_f64(), b"signalCode", TAG_NULL_F64);
    (!handled).then(|| err.get_nanbox_f64())
}

/// Throw `unhandled` as an uncaught exception. Called only once every handle
/// scope in the emitting frame has dropped, so the throw skips no cleanup.
pub(super) fn throw_if_unhandled(unhandled: Option<f64>) {
    if let Some(err) = unhandled {
        crate::exception::js_throw(err);
    }
}

pub(super) fn finish_outputs(cp: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cp = scope.root_nanbox_f64(cp);
    let stdio = scope.root_nanbox_f64(cp_get_field(cp.get_nanbox_f64(), b"stdio"));
    let count = cp_array_ptr(stdio.get_nanbox_f64())
        .map(|array| crate::array::js_array_length(array))
        .unwrap_or(0);
    // fd 0 is writable stdin; every other pipe built by spawn is readable.
    // Ignored/inherited fds are null and must not receive synthetic events.
    for fd in 1..count {
        let Some(array) = cp_array_ptr(stdio.get_nanbox_f64()) else {
            break;
        };
        let stream = crate::array::js_array_get_f64(array, fd);
        finish_output(stream);
    }
}

fn finish_output(stream: f64) {
    if cp_object_ptr(stream).is_none() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    if cp_get_field(stream.get_nanbox_f64(), b"closed").to_bits() == TAG_TRUE_F64.to_bits() {
        return;
    }
    cp_readable_end(stream.get_nanbox_f64());
    cp_set_field(stream.get_nanbox_f64(), b"destroyed", TAG_TRUE_F64);
    cp_set_field(stream.get_nanbox_f64(), b"closed", TAG_TRUE_F64);
    cp_emit(stream.get_nanbox_f64(), "close", &[]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_output_retains_end_and_closed_state() {
        cp_register_arities();
        let scope = crate::gc::RuntimeHandleScope::new();
        let stream = scope.root_nanbox_f64(cp_build_readable());
        for _ in 0..2 {
            finish_output(stream.get_nanbox_f64());
            assert_eq!(
                cp_get_field(stream.get_nanbox_f64(), b"readable").to_bits(),
                TAG_FALSE_F64.to_bits()
            );
            for key in [b"readableEnded".as_slice(), b"destroyed", b"closed"] {
                assert_eq!(
                    cp_get_field(stream.get_nanbox_f64(), key).to_bits(),
                    TAG_TRUE_F64.to_bits()
                );
            }
        }
    }

    extern "C" fn swallow_error(_closure: *const ClosureHeader, _err: f64) -> f64 {
        cp_undefined()
    }

    /// A failed-spawn ChildProcess stand-in: just the fields the emit reads.
    fn failed_child<'s>(scope: &'s crate::gc::RuntimeHandleScope) -> crate::gc::RuntimeHandle<'s> {
        let cp = scope.root_nanbox_f64(cp_box_ptr(crate::object::js_object_alloc(0, 4).cast()));
        let err = cp_make_error(
            "spawn /missing ENOENT",
            &[("code", cp_box_string("ENOENT"))],
        );
        cp_set_field(cp.get_nanbox_f64(), b"__cpError", err);
        cp
    }

    /// #10730: with no `error` listener the error comes back to be thrown.
    /// Before the fix the emit reported nothing, so the error was dropped.
    #[test]
    fn spawn_error_without_listener_is_unhandled() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let cp = failed_child(&scope);
        let err = cp_get_field(cp.get_nanbox_f64(), b"__cpError");
        let unhandled = emit_spawn_error(cp.get_nanbox_f64())
            .expect("an error event nobody listens for must be thrown");
        assert_eq!(
            unhandled.to_bits(),
            err.to_bits(),
            "Node rethrows the same object"
        );
    }

    #[test]
    fn spawn_error_with_listener_is_handled() {
        crate::closure::js_register_closure_arity(swallow_error as *const u8, 1);
        let scope = crate::gc::RuntimeHandleScope::new();
        let cp = failed_child(&scope);
        let listener = crate::closure::js_closure_alloc(swallow_error as *const u8, 0);
        super::super::emitter::cp_register(
            cp.get_nanbox_f64(),
            cp_box_string("error"),
            cp_box_ptr(listener.cast()),
        );
        assert!(emit_spawn_error(cp.get_nanbox_f64()).is_none());
    }

    #[test]
    fn absent_failed_output_is_ignored() {
        finish_output(TAG_NULL_F64);
        finish_output(cp_undefined());
    }
}
