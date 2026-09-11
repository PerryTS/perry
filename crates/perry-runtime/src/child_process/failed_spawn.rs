//! A failed spawn has no live reactor entry to deliver output-pipe EOF.
//! Finish those empty streams after the child error and before child close.
use super::*;

/// Schedule close only after delivering the error. A close timer armed at spawn
/// time can already be overdue before the error's setImmediate callback runs.
/// Keep fork's separate failure contract unchanged by using this for spawn only.
pub(super) extern "C" fn emit_error_then_close(closure: *const ClosureHeader) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cp = scope.root_nanbox_f64(cp_this(closure));
    reactor::cp_emit_spawn_error(closure);
    let close = crate::closure::js_closure_alloc(reactor::cp_emit_spawn_close as *const u8, 1);
    crate::closure::js_closure_set_capture_ptr(close, 0, cp.get_nanbox_f64().to_bits() as i64);
    crate::timer::js_set_timeout_callback(close as i64, 1.0);
    cp_undefined()
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

    #[test]
    fn absent_failed_output_is_ignored() {
        finish_output(TAG_NULL_F64);
        finish_output(cp_undefined());
    }
}
