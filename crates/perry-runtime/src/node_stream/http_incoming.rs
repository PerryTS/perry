//! node's `IncomingMessage.prototype` methods `_read`, `_destroy` and `_dump`,
//! each its own native function, and the server's finish-time check that
//! calls `_dump` (node's `resOnFinish`).
//!
//! A request born on its layout whose sealed prototype chain is unwritten
//! ([`pristine`]) has each own key in its layout slot and inherits exactly
//! the runtime's methods and accessors: the bodies read and store slots and
//! call the stream's own operations. Any other receiver runs node's body
//! through ordinary `[[Get]]`, `[[Set]]` and method calls, so a program's
//! replacement (on the prototype, on a subclass or on the instance) is the
//! one that runs.
use super::*;
use crate::object::runtime_layouts::{incoming_slot as s, readable_slot};
use crate::object::ObjectHeader;

/// `receiver` as a request born on the IncomingMessage layout's keys whose
/// prototype is the sealed `IncomingMessage.prototype` with every prototype
/// of its chain unwritten since sealing. Each write to one of them (a store,
/// a define, a delete, a relink) sets its shadow bit, so this is also the
/// proof that every method and accessor the request inherits is the
/// runtime's own.
pub(crate) fn pristine(receiver: f64) -> Option<*mut ObjectHeader> {
    let obj = crate::object::field_get_set::runtime_read_site::object_receiver(receiver)?;
    // SAFETY: a live object header.
    unsafe {
        let layout = crate::node_stream::instance_birth::instance_layout(obj)?;
        if (*layout).layout_id != crate::object::runtime_layouts::INCOMING_MESSAGE::WORD {
            return None;
        }
        let proto = crate::object::shapes::object_prototype_word(obj);
        crate::node_stream::instance_birth::sealed_chain(f64::from_bits(proto), layout)
            .then_some(obj)
    }
}

/// The word in `slot` of a [`pristine`] request (`undefined` for the hole).
///
/// # Safety
/// `obj` is a live request on the IncomingMessage layout.
unsafe fn slot(obj: *mut ObjectHeader, slot: u32) -> f64 {
    let bits = *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64)
        .add(slot as usize);
    f64::from_bits(if bits == crate::value::TAG_HOLE {
        TAG_UNDEFINED
    } else {
        bits
    })
}

/// Store a present plain data key of a [`pristine`] request.
///
/// # Safety
/// `obj` is a live request on the IncomingMessage layout holding `slot`.
unsafe fn set_slot(obj: *mut ObjectHeader, slot: u32, value: f64) {
    crate::object::store_object_field_slot(obj, slot as usize, value.to_bits());
}

fn truthy(value: f64) -> bool {
    crate::value::js_is_truthy(value) != 0
}

fn flag(on: bool) -> f64 {
    f64::from_bits(if on { TAG_TRUE } else { TAG_FALSE })
}

fn get(this: f64, name: &'static [u8]) -> f64 {
    // SAFETY: a live value and a static key.
    unsafe { crate::runtime_state_key::js_runtime_state_key_get(this, name.as_ptr(), name.len()) }
}

fn set(this: f64, name: &'static [u8], value: f64) {
    // SAFETY: as above.
    unsafe {
        crate::runtime_state_key::js_runtime_state_key_set(this, name.as_ptr(), name.len(), value)
    }
}

fn call(this: f64, name: &'static [u8], args: &[f64]) -> f64 {
    // SAFETY: as above; `args` outlives the call.
    unsafe {
        crate::runtime_state_key::js_runtime_state_key_call(
            this,
            name.as_ptr(),
            name.len(),
            args.as_ptr(),
            args.len(),
        )
    }
}

/// A numeric handle receiver (an HTTP/2 compat request): the handle's own
/// dispatch by the captured name.
fn handle_receiver(this: crate::closure::JsThis) -> bool {
    JSValue::from_bits(this.as_f64().to_bits()).is_handle()
}

/// node's `_dump()`: `if (!this._dumped) { this._dumped = true;
/// this.removeAllListeners('data'); this.resume(); }`.
pub(crate) fn dump(receiver: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(receiver);
    let data = scope.root_nanbox_f64(literal_string_value(b"data"));
    if let Some(obj) = pristine(this.get_nanbox_f64()) {
        // SAFETY: a pristine request.
        unsafe {
            if truthy(slot(obj, s::DUMPED)) {
                return;
            }
            set_slot(obj, s::DUMPED, flag(true));
        }
        event_emitter::remove_all_stream_listeners(
            this.get_nanbox_f64(),
            data.get_nanbox_f64(),
            false,
        );
    } else {
        if truthy(get(this.get_nanbox_f64(), b"_dumped")) {
            return;
        }
        set(this.get_nanbox_f64(), b"_dumped", flag(true));
        call(
            this.get_nanbox_f64(),
            b"removeAllListeners",
            &[data.get_nanbox_f64()],
        );
    }
    // A `'removeListener'` listener may have written the chain: decide again.
    if pristine(this.get_nanbox_f64()).is_some() {
        resume_readable_stream(this.get_nanbox_f64());
    } else {
        call(this.get_nanbox_f64(), b"resume", &[]);
    }
}

extern "C" fn im_dump(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    a: f64,
    b: f64,
    d: f64,
) -> f64 {
    if handle_receiver(this) {
        return crate::object::node_http_prototypes::js_node_http_method_fallback(
            closure, this, a, b, d,
        );
    }
    dump(this.as_f64());
    f64::from_bits(TAG_UNDEFINED)
}

/// node's `_read(n)`: the first read marks the message consumed; the body is
/// already buffered, so there is nothing to start.
extern "C" fn im_read(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    a: f64,
    b: f64,
    d: f64,
) -> f64 {
    if handle_receiver(this) {
        return crate::object::node_http_prototypes::js_node_http_method_fallback(
            closure, this, a, b, d,
        );
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this.as_f64());
    let (consuming, state) = match pristine(this.get_nanbox_f64()) {
        // SAFETY: a pristine request.
        Some(obj) => unsafe { (slot(obj, s::CONSUMING), None) },
        None => (get(this.get_nanbox_f64(), b"_consuming"), None::<f64>),
    };
    if truthy(consuming) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let state = scope.root_nanbox_f64(state.unwrap_or_else(|| {
        match pristine(this.get_nanbox_f64()) {
            // SAFETY: a pristine request.
            Some(obj) => unsafe { slot(obj, readable_slot::STATE) },
            None => get(this.get_nanbox_f64(), b"_readableState"),
        }
    }));
    if JSValue::from_bits(state.get_nanbox_u64()).is_pointer() {
        set(state.get_nanbox_f64(), b"readingMore", flag(false));
    }
    match pristine(this.get_nanbox_f64()) {
        // SAFETY: a pristine request.
        Some(obj) => unsafe { set_slot(obj, s::CONSUMING, flag(true)) },
        None => set(this.get_nanbox_f64(), b"_consuming", flag(true)),
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// node's `_destroy(err, cb)`: an incomplete message is aborted (`aborted`
/// set, `'aborted'` emitted), then node's `onError`: the error goes to `cb`
/// only when an `'error'` listener is attached, else `cb()`.
extern "C" fn im_destroy(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    err: f64,
    cb: f64,
    d: f64,
) -> f64 {
    if handle_receiver(this) {
        return crate::object::node_http_prototypes::js_node_http_method_fallback(
            closure, this, err, cb, d,
        );
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this.as_f64());
    let err = scope.root_nanbox_f64(err);
    let cb = scope.root_nanbox_f64(cb);
    let r = || this.get_nanbox_f64();
    // `!this.readableEnded || !this.complete`, in that order.
    let incomplete = match pristine(r()) {
        // SAFETY: a pristine request; `readableEnded` is Readable's getter.
        Some(obj) => {
            !has_truthy_hidden(r(), Slot::EndEmitted) || unsafe { !truthy(slot(obj, s::COMPLETE)) }
        }
        None => !truthy(get(r(), b"readableEnded")) || !truthy(get(r(), b"complete")),
    };
    if incomplete {
        abort(r());
    }
    if JSValue::from_bits(cb.get_nanbox_u64()).is_pointer() {
        let event = scope.root_nanbox_f64(literal_string_value(b"error"));
        let listened = if pristine(r()).is_some() {
            event_emitter::stream_listener_count_for_event(r(), event.get_nanbox_f64()) > 0
        } else {
            let n = call(r(), b"listenerCount", &[event.get_nanbox_f64()]);
            JSValue::from_bits(n.to_bits()).is_number() && n > 0.0
        };
        let undef = f64::from_bits(TAG_UNDEFINED);
        if listened {
            crate::object::property_ops::common::call_with_this_and_args(
                cb.get_nanbox_f64(),
                undef,
                &[err.get_nanbox_f64()],
            );
        } else {
            crate::object::property_ops::common::call_with_this_and_args(
                cb.get_nanbox_f64(),
                undef,
                &[],
            );
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `if (!this.aborted) { this.aborted = true; this.emit('aborted'); }`.
fn abort(receiver: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(receiver);
    let event = scope.root_nanbox_f64(literal_string_value(b"aborted"));
    if let Some(obj) = pristine(this.get_nanbox_f64()) {
        // SAFETY: a pristine request.
        unsafe {
            if truthy(slot(obj, s::ABORTED)) {
                return;
            }
            set_slot(obj, s::ABORTED, flag(true));
        }
        event_emitter::emit_stream_event(this.get_nanbox_f64(), event.get_nanbox_f64(), &[]);
        return;
    }
    if truthy(get(this.get_nanbox_f64(), b"aborted")) {
        return;
    }
    set(this.get_nanbox_f64(), b"aborted", flag(true));
    call(this.get_nanbox_f64(), b"emit", &[event.get_nanbox_f64()]);
}

/// node's `resOnFinish` check, `_dump()` unless a reader consumes the body,
/// a `resume()` is scheduled, or the request is destroyed.
pub(crate) fn dump_unread(receiver: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(receiver);
    let r = || this.get_nanbox_f64();
    if let Some(obj) = pristine(r()) {
        // SAFETY: a pristine request: own fields in their slots, and
        // `destroyed` is Readable's getter.
        let (consuming, state) =
            unsafe { (slot(obj, s::CONSUMING), slot(obj, readable_slot::STATE)) };
        if truthy(consuming) || stream_destroyed(r()) {
            return;
        }
        let state = scope.root_nanbox_f64(state);
        if JSValue::from_bits(state.get_nanbox_u64()).is_pointer()
            && truthy(get(state.get_nanbox_f64(), b"resumeScheduled"))
        {
            return;
        }
        if pristine(r()).is_some() {
            dump(r());
        } else {
            call(r(), b"_dump", &[]);
        }
        return;
    }
    let state = scope.root_nanbox_f64(get(r(), b"_readableState"));
    let resume_scheduled = JSValue::from_bits(state.get_nanbox_u64()).is_pointer()
        && truthy(get(state.get_nanbox_f64(), b"resumeScheduled"));
    if !truthy(get(r(), b"_consuming")) && !resume_scheduled && !truthy(get(r(), b"destroyed")) {
        call(r(), b"_dump", &[]);
    }
}

/// [`dump_unread`] for the http extension's response finish.
///
/// # Safety
/// FFI entry: `this` is a live JS value.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_incoming_message_dump_unread(this: f64) {
    if crate::object::field_get_set::runtime_read_site::object_receiver(this).is_some() {
        dump_unread(this);
    }
}

#[rustfmt::skip] crate::realm_decl! { info crate::fn_info!(export static IM_DUMP_INFO = crate::node_stream::http_incoming::im_dump, 3; with_declared(3)); realm { "node:http.IncomingMessage.prototype._dump", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_incomingmessage_prototype_dump_metadata); } }
#[rustfmt::skip] crate::realm_decl! { info crate::fn_info!(export static IM_READ_INFO = crate::node_stream::http_incoming::im_read, 3; with_declared(3)); realm { "node:http.IncomingMessage.prototype._read", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_incomingmessage_prototype_read_metadata); } }
#[rustfmt::skip] crate::realm_decl! { info crate::fn_info!(export static IM_DESTROY_INFO = crate::node_stream::http_incoming::im_destroy, 3; with_declared(3)); realm { "node:http.IncomingMessage.prototype._destroy", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_node_http_incomingmessage_prototype_destroy_metadata); } }

/// The method body of `IncomingMessage.prototype[name]` when it is one of
/// this module's, for the prototype's build.
pub(crate) fn method_info(name: &str) -> Option<*const crate::closure::JsFunctionInfo> {
    let info: &crate::closure::JsFunctionInfo = match name {
        "_dump" => &IM_DUMP_INFO,
        "_read" => &IM_READ_INFO,
        "_destroy" => &IM_DESTROY_INFO,
        _ => return None,
    };
    Some(info as *const _)
}
