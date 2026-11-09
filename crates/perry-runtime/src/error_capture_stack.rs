//! Captured frame ownership for Error.captureStackTrace's lazy accessor.

use super::*;

extern "C" fn error_stack_lazy_getter(
    closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver = scope.root_nanbox_f64(this.as_f64());
        // Copy the encoded bytes before any JS allocation can move the getter
        // or its captured string. Name resolution remains lazy.
        let bits = crate::closure::js_closure_get_capture_bits(closure, 0);
        let payload = read_string_header_owned(crate::value::js_nanbox_get_pointer(f64::from_bits(
            bits,
        )) as *const StringHeader);
        if let Some(prep) = error_prepare_stack_trace_override() {
            let prep = scope.root_nanbox_f64(prep);
            let structured = build_structured_stack(10);
            return crate::closure::js_closure_call2(
                crate::value::js_nanbox_get_pointer(prep.get_nanbox_f64())
                    as *const crate::closure::ClosureHeader,
                crate::closure::plain_call_receiver(),
                receiver.get_nanbox_f64(),
                structured,
            );
        }
        let text = join_stack("Error", &frames_payload_to_lines(payload.as_bytes()));
        let value = js_string_from_bytes(text.as_ptr(), text.len() as u32);
        crate::value::js_nanbox_string(value as i64)
    }
}

pub(super) unsafe fn install_lazy_stack_accessor(target: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(target);
    let payload = capture_frames_payload();
    let frame = js_string_from_bytes(payload.as_ptr(), payload.len() as u32);
    if frame.is_null() {
        return;
    }
    let frame = scope.root_string_ptr(frame);
    let getter = crate::closure::js_closure_alloc(
        crate::fn_info!(error_stack_lazy_getter, 0; with_declared(0)),
        1,
    );
    if getter.is_null() {
        return;
    }
    let getter = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(getter as i64));
    let key = scope.root_string_ptr(js_string_from_bytes(b"stack".as_ptr(), 5));
    frame.with_const_ptr::<StringHeader, _>(|frame| {
        crate::closure::js_closure_set_capture_bits(
            crate::value::js_nanbox_get_pointer(getter.get_nanbox_f64())
                as *mut crate::closure::ClosureHeader,
            0,
            crate::value::js_nanbox_string(frame as i64).to_bits(),
        );
    });
    // Key insertion can collect. Re-read both entry pointers after it.
    let (((), stack_key), target) = receiver.across_nanbox(|| {
        key.across_const::<StringHeader, _>(|| {
            key.with_const_ptr::<StringHeader, _>(|key| {
                crate::object::ensure_key_in_keys_array(
                    crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64())
                        as *mut crate::object::ObjectHeader,
                    key,
                );
            })
        })
    });
    let name = read_string_header_owned(stack_key);
    crate::object::set_builtin_accessor_descriptor(
        crate::value::js_nanbox_get_pointer(target) as usize,
        name,
        crate::object::AccessorDescriptor {
            get: getter.get_nanbox_f64().to_bits(),
            set: 0,
        },
        crate::object::PropertyAttrs::new(true, false, true),
    );
}
