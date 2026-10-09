//! Event delivery through ordinary emitter properties and nextTick captures.

use super::payload_transport::{boxed_addr, raw_owner, undefined};
use perry_ffi::{JsThis, JsValue, RawClosureHeader, TransientRootScope};

extern "C" {
    fn js_perry_event_emit_span_rooted(
        scope: usize,
        resource: usize,
        owner: usize,
        event: *const u8,
        event_len: usize,
        args: *const f64,
        len: usize,
    ) -> f64;
    fn js_node_stream_listener_count_fast(owner: f64, event: *const u8, len: usize) -> i64;
    fn js_string_new_sso(data: *const u8, len: u32) -> f64;
    fn js_perry_event_emit_span_in_resource(
        resource: f64,
        owner: f64,
        event: *const u8,
        event_len: usize,
        args: *const f64,
        len: usize,
    ) -> f64;
    fn js_perry_event_emit_in_resource(resource: f64, owner: f64, event: f64, args: f64) -> f64;
    fn js_perry_event_call_in_resource(
        resource: f64,
        callback: f64,
        receiver: f64,
        args: *const f64,
        len: usize,
    ) -> f64;
    fn js_node_stream_method_listener_count(owner: i64, event: f64) -> f64;
    fn js_queue_next_tick(callback: i64);
}

pub(crate) fn string(text: &str) -> f64 {
    unsafe { js_string_new_sso(text.as_ptr(), text.len() as u32) }
}

/// The array is a traced JS object, including during allocation and callback
/// reentry. No Rust snapshot of callbacks or argument pointers is retained.
fn argument_array(scope: &TransientRootScope, args: &[f64]) -> perry_ffi::TransientRootedAddr {
    let args: Vec<_> = args.iter().map(|&arg| scope.root_nanbox(arg)).collect();
    let mut array = scope.root_addr(unsafe { perry_ffi::js_array_alloc(args.len() as u32) } as i64);
    for arg in args {
        let updated = unsafe {
            perry_ffi::js_array_push(
                array.get() as *mut perry_ffi::ArrayHeader,
                JsValue::from_bits(arg.get().to_bits()),
            )
        };
        array = scope.root_addr(updated as i64);
    }
    array
}

pub(crate) fn emit(owner: f64, event: &str, args: &[f64]) -> f64 {
    // Provider resolution is a callback-free read of an ordinary state edge.
    emit_in(super::payload_provider::resource(owner), owner, event, args)
}

pub(crate) fn emit_in(resource: f64, owner: f64, event: &str, args: &[f64]) -> f64 {
    unsafe {
        js_perry_event_emit_span_in_resource(
            resource,
            owner,
            event.as_ptr(),
            event.len(),
            args.as_ptr(),
            args.len(),
        )
    }
}

pub(crate) fn emit_rooted(
    scope: &TransientRootScope,
    resource: &perry_ffi::TransientRootedNanbox,
    owner: &perry_ffi::TransientRootedNanbox,
    event: &str,
    args: &[f64],
) -> f64 {
    unsafe {
        js_perry_event_emit_span_rooted(
            scope.runtime_token(),
            resource.runtime_token(),
            owner.runtime_token(),
            event.as_ptr(),
            event.len(),
            args.as_ptr(),
            args.len(),
        )
    }
}

pub(crate) fn listener_count(owner: f64, event: &str) -> usize {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let event = string(event);
    let result = unsafe { js_node_stream_method_listener_count(raw_owner(owner.get()), event) };
    JsValue::from_bits(result.to_bits()).to_number().max(0.0) as usize
}

/// Ordinary own-data emitter shapes need neither allocation nor field roots.
pub(crate) fn listener_count_fast(owner: f64, event: &str) -> Option<usize> {
    let count = unsafe { js_node_stream_listener_count_fast(owner, event.as_ptr(), event.len()) };
    (count >= 0).then_some(count as usize)
}

unsafe extern "C" fn emit_tick(closure: *const RawClosureHeader, _: JsThis) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    let event = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 1));
    let args = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 2));
    let resource = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 3));
    let terminal = perry_ffi::closure_capture_f64(closure, 4) == 1.0;
    js_perry_event_emit_in_resource(resource.get(), owner.get(), event.get(), args.get());
    if terminal {
        super::payload_provider::retire(resource.get());
    }
    undefined()
}

/// Synthetic events have no driver completion to keep the owner alive. The
/// nextTick closure supplies the ordinary traced owner edge instead.
pub(crate) fn queue_emit(owner: f64, event: &str, args: &[f64]) {
    queue_emit_with(owner, event, args, false);
}
fn queue_emit_with(owner: f64, event: &str, args: &[f64], terminal: bool) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let args: Vec<_> = args.iter().map(|&arg| scope.root_nanbox(arg)).collect();
    let resource = scope.root_nanbox(super::payload_provider::resource(owner.get()));
    let args: Vec<_> = args.iter().map(|arg| arg.get()).collect();
    queue_emit_in(resource.get(), owner.get(), event, &args, terminal);
}
pub(crate) fn queue_emit_in(resource: f64, owner: f64, event: &str, args: &[f64], terminal: bool) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let resource = scope.root_nanbox(resource);
    let args = argument_array(&scope, args);
    let event = scope.root_nanbox(string(event));
    let callback = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(emit_tick, 0; with_flags(perry_ffi::FN_BUILTIN)),
        5,
    ) as i64);
    unsafe {
        let callback_ptr = callback.get() as *mut RawClosureHeader;
        perry_ffi::set_closure_capture_f64(callback_ptr, 0, owner.get());
        perry_ffi::set_closure_capture_f64(callback_ptr, 1, event.get());
        perry_ffi::set_closure_capture_f64(callback_ptr, 2, boxed_addr(args.get()));
        perry_ffi::set_closure_capture_f64(callback_ptr, 3, resource.get());
        perry_ffi::set_closure_capture_f64(callback_ptr, 4, if terminal { 1.0 } else { 0.0 });
        js_queue_next_tick(callback.get());
    }
}

unsafe extern "C" fn call_tick(closure: *const RawClosureHeader, _: JsThis) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    let callback = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 1));
    let array = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 2));
    let array_ptr =
        JsValue::from_bits(array.get().to_bits()).as_pointer::<perry_ffi::ArrayHeader>();
    let len = perry_ffi::js_array_length(array_ptr);
    let args: Vec<_> = (0..len)
        .map(|index| {
            scope.root_nanbox(f64::from_bits(
                perry_ffi::js_array_get(array_ptr, index).bits(),
            ))
        })
        .collect();
    let args: Vec<_> = args.iter().map(|arg| arg.get()).collect();
    let resource = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 3));
    call_in(resource.get(), callback.get(), JsThis::UNDEFINED, &args);
    let _ = owner;
    undefined()
}

pub(crate) fn queue_call(owner: f64, callback: f64, args: &[f64]) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let callback = scope.root_nanbox(callback);
    let args = argument_array(&scope, args);
    let resource = scope.root_nanbox(super::payload_provider::resource(owner.get()));
    let job = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(call_tick, 0; with_flags(perry_ffi::FN_BUILTIN)),
        4,
    ) as i64);
    unsafe {
        let job_ptr = job.get() as *mut RawClosureHeader;
        perry_ffi::set_closure_capture_f64(job_ptr, 0, owner.get());
        perry_ffi::set_closure_capture_f64(job_ptr, 1, callback.get());
        perry_ffi::set_closure_capture_f64(job_ptr, 2, boxed_addr(args.get()));
        perry_ffi::set_closure_capture_f64(job_ptr, 3, resource.get());
        js_queue_next_tick(job.get());
    }
}

pub(crate) fn call(callback: f64, this: JsThis, args: &[f64]) -> f64 {
    call_in(undefined(), callback, this, args)
}
pub(crate) fn call_in(resource: f64, callback: f64, this: JsThis, args: &[f64]) -> f64 {
    // The runtime boundary roots every input before any allocation or hook.
    unsafe {
        js_perry_event_call_in_resource(
            resource,
            callback,
            this.as_f64(),
            args.as_ptr(),
            args.len(),
        )
    }
}
