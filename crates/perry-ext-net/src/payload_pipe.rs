//! Pipe callbacks and destination edges live in the owner's ordinary JS state.

use super::{payload_events as events, payload_socket as socket, payload_transport as p};
use perry_ffi::{ArrayHeader, JsThis, JsValue, RawClosureHeader, TransientRootScope};

extern "C" {
    fn js_perry_event_get(owner: f64, key: f64) -> f64;
    fn js_node_stream_method_on(owner: i64, event: f64, callback: f64) -> f64;
    fn js_node_stream_method_remove_listener(owner: i64, event: f64, callback: f64) -> f64;
    fn js_object_alloc_null_proto(class_id: u32, fields: u32) -> *mut perry_ffi::ObjectHeader;
}
fn array(value: f64) -> *mut ArrayHeader {
    JsValue::from_bits(value.to_bits()).as_pointer()
}

fn get(owner: f64, key: &str) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let key = scope.root_nanbox(events::string(key));
    unsafe { js_perry_event_get(owner.get(), key.get()) }
}
fn method(owner: f64, key: &str, args: &[f64]) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let args: Vec<_> = args.iter().map(|&arg| scope.root_nanbox(arg)).collect();
    let callback = scope.root_nanbox(get(owner.get(), key));
    if !socket::is_callback(callback.get()) {
        return p::undefined();
    }
    let args: Vec<_> = args.iter().map(|arg| arg.get()).collect();
    events::call(callback.get(), JsThis::from_f64(owner.get()), &args)
}
unsafe extern "C" fn data(closure: *const RawClosureHeader, _: JsThis, chunk: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let source = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    let dest = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 1));
    let chunk = scope.root_nanbox(chunk);
    let link = socket::link(source.get());
    let snapshot = super::payload_io::snapshot(link);
    let result = method(dest.get(), "write", &[chunk.get()]);
    if result.to_bits() == JsValue::FALSE.bits()
        && snapshot
            .as_ref()
            .is_some_and(|snapshot| super::payload_io::matches(link, snapshot))
    {
        socket::set_paused(source.get(), true);
    }
    p::undefined()
}
unsafe extern "C" fn end(closure: *const RawClosureHeader, _: JsThis) -> f64 {
    let scope = TransientRootScope::enter();
    let dest = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    method(dest.get(), "end", &[]);
    p::undefined()
}
unsafe extern "C" fn drain(closure: *const RawClosureHeader, _: JsThis) -> f64 {
    let scope = TransientRootScope::enter();
    let source = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    // This is an ordinary stream control callback. A released Socket remains
    // the same owner; resume must not attach native state or create a handle.
    socket::set_paused(source.get(), false);
    p::undefined()
}
fn callback(info: &'static perry_ffi::JsFunctionInfo, captures: &[f64]) -> f64 {
    let scope = TransientRootScope::enter();
    let captures: Vec<_> = captures
        .iter()
        .map(|&value| scope.root_nanbox(value))
        .collect();
    let callback = scope.root_addr(perry_ffi::alloc_closure(info, captures.len() as u32) as i64);
    for (index, value) in captures.iter().enumerate() {
        unsafe {
            perry_ffi::set_closure_capture_f64(
                callback.get() as *mut RawClosureHeader,
                index as u32,
                value.get(),
            );
        }
    }
    p::boxed_addr(callback.get())
}
fn listen(owner: f64, event: &str, callback: f64, remove: bool) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let callback = scope.root_nanbox(callback);
    let event = scope.root_nanbox(events::string(event));
    unsafe {
        if remove {
            js_node_stream_method_remove_listener(
                p::raw_owner(owner.get()),
                event.get(),
                callback.get(),
            );
        } else {
            js_node_stream_method_on(p::raw_owner(owner.get()), event.get(), callback.get());
        }
    }
}
pub(crate) fn pipe(owner: f64, dest: f64, options: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let dest = scope.root_nanbox(dest);
    let options = scope.root_nanbox(options);
    socket::link(owner.get());
    let state = scope.root_nanbox(socket::state(owner.get()));
    let ends = !JsValue::from_bits(options.get().to_bits()).is_pointer()
        || get(options.get(), "end").to_bits() != JsValue::FALSE.bits();
    let record = scope.root_nanbox(p::boxed_addr(
        unsafe { js_object_alloc_null_proto(0, 4) } as i64
    ));
    let on_data = scope.root_nanbox(callback(
        perry_ffi::js_function_info!(data, 1; with_flags(perry_ffi::FN_BUILTIN)),
        &[owner.get(), dest.get()],
    ));
    let on_end = scope.root_nanbox(if ends {
        callback(
            perry_ffi::js_function_info!(end, 0; with_flags(perry_ffi::FN_BUILTIN)),
            &[dest.get()],
        )
    } else {
        p::undefined()
    });
    let on_drain = scope.root_nanbox(callback(
        perry_ffi::js_function_info!(drain, 0; with_flags(perry_ffi::FN_BUILTIN)),
        &[owner.get()],
    ));
    p::own_set(record.get(), "dest", dest.get());
    p::own_set(record.get(), "data", on_data.get());
    p::own_set(record.get(), "end", on_end.get());
    p::own_set(record.get(), "drain", on_drain.get());
    let mut records = scope.root_nanbox(p::own_get(state.get(), "pipes"));
    if !JsValue::from_bits(records.get().to_bits()).is_pointer() {
        records = scope.root_nanbox(p::boxed_addr(unsafe { perry_ffi::js_array_alloc(0) } as i64));
    }
    let updated = unsafe {
        perry_ffi::js_array_push(
            array(records.get()),
            JsValue::from_bits(record.get().to_bits()),
        )
    };
    p::own_set(state.get(), "pipes", p::boxed_addr(updated as i64));
    listen(owner.get(), "data", on_data.get(), false);
    if ends {
        listen(owner.get(), "end", on_end.get(), false);
    }
    listen(dest.get(), "drain", on_drain.get(), false);
    events::emit(dest.get(), "pipe", &[owner.get()]);
    socket::flow(owner.get());
    dest.get()
}
pub(crate) fn unpipe(owner: f64, dest: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let dest = scope.root_nanbox(dest);
    socket::link(owner.get());
    let state = scope.root_nanbox(socket::state(owner.get()));
    let records = scope.root_nanbox(p::own_get(state.get(), "pipes"));
    if !JsValue::from_bits(records.get().to_bits()).is_pointer() {
        return owner.get();
    }
    let count = unsafe { perry_ffi::js_array_length(array(records.get())) };
    let values: Vec<_> = (0..count)
        .map(|i| {
            scope.root_nanbox(f64::from_bits(unsafe {
                perry_ffi::js_array_get(array(records.get()), i).bits()
            }))
        })
        .collect();
    let mut keep = scope.root_addr(unsafe { perry_ffi::js_array_alloc(count) } as i64);
    let mut removed = Vec::new();
    for value in values {
        let target = scope.root_nanbox(p::own_get(value.get(), "dest"));
        if JsValue::from_bits(dest.get().to_bits()).is_undefined()
            || target.get().to_bits() == dest.get().to_bits()
        {
            removed.push(value);
        } else {
            keep = scope.root_addr(unsafe {
                perry_ffi::js_array_push(
                    keep.get() as *mut ArrayHeader,
                    JsValue::from_bits(value.get().to_bits()),
                )
            } as i64);
        }
    }
    // Publish removal before any destination listener can pipe again.
    p::own_set(state.get(), "pipes", p::boxed_addr(keep.get()));
    for record in removed {
        let target = scope.root_nanbox(p::own_get(record.get(), "dest"));
        let on_data = scope.root_nanbox(p::own_get(record.get(), "data"));
        let on_end = scope.root_nanbox(p::own_get(record.get(), "end"));
        let on_drain = scope.root_nanbox(p::own_get(record.get(), "drain"));
        listen(owner.get(), "data", on_data.get(), true);
        if socket::is_callback(on_end.get()) {
            listen(owner.get(), "end", on_end.get(), true);
        }
        listen(target.get(), "drain", on_drain.get(), true);
        events::emit(target.get(), "unpipe", &[owner.get()]);
    }
    owner.get()
}
