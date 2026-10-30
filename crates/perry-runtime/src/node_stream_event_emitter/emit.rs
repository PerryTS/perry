//! Delivering one event: the arguments of a single `emit`, rooted for its
//! dispatch window, the listener dispatch itself, and the unhandled-`error`
//! domain routing that a failed dispatch falls through to. Split out of the
//! emitter's surface (construction, listener registry, capture policy).
use super::*;

/// The arguments of one `emit`, rooted for its dispatch window. Up to
/// [`INLINE_ARGS`] are held without a heap allocation.
const INLINE_ARGS: usize = 4;

pub(crate) enum RootedArgs<'scope> {
    Inline(
        [Option<crate::gc::RuntimeHandle<'scope>>; INLINE_ARGS],
        usize,
    ),
    Heap(Vec<crate::gc::RuntimeHandle<'scope>>),
}

impl<'scope> RootedArgs<'scope> {
    pub(crate) fn new(scope: &'scope crate::gc::RuntimeHandleScope, args: &[f64]) -> Self {
        if args.len() > INLINE_ARGS {
            return RootedArgs::Heap(scope.root_nanbox_f64_slice(args));
        }
        let mut handles = [None, None, None, None];
        for (handle, arg) in handles.iter_mut().zip(args) {
            *handle = Some(scope.root_nanbox_f64(*arg));
        }
        RootedArgs::Inline(handles, args.len())
    }

    /// The arguments as they are now (a collection may have moved them).
    pub(crate) fn with_live<R>(&self, f: impl FnOnce(&[f64]) -> R) -> R {
        match self {
            RootedArgs::Inline(handles, len) => {
                let mut live = [0f64; INLINE_ARGS];
                for (slot, handle) in live.iter_mut().zip(handles.iter().flatten()) {
                    *slot = handle.get_nanbox_f64();
                }
                f(&live[..*len])
            }
            RootedArgs::Heap(handles) => f(
                &crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(handles),
            ),
        }
    }
}

/// node's `emit(type, ...args)`.
pub(crate) fn emit_stream_event(stream: f64, event: f64, args: &[f64]) -> f64 {
    // #10600: a listener can allocate enough to trigger a moving collection,
    // so the receiver, the event, the arguments and the listener snapshot are
    // all rooted for the whole dispatch window and re-read before each call.
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream_h = scope.root_nanbox_f64(stream);
    let event_h = scope.root_nanbox_f64(event);
    let arg_handles = RootedArgs::new(&scope, args);

    emit_stream_event_rooted(&scope, &stream_h, &event_h, &arg_handles)
}

/// Native events share their dispatch roots with the async-provider boundary.
pub(crate) fn emit_stream_event_rooted(
    scope: &crate::gc::RuntimeHandleScope,
    stream_h: &crate::gc::RuntimeHandle<'_>,
    event_h: &crate::gc::RuntimeHandle<'_>,
    arg_handles: &RootedArgs<'_>,
) -> f64 {
    let stream = stream_h.get_nanbox_f64();
    let event = event_h.get_nanbox_f64();
    let is_error = super::super::string_value_eq(event, b"error");
    if !is_error {
        // The shapes' answer for `this._events[type]`: no listener, or one.
        if let Some(handler) = listeners_of_fast(stream, event) {
            if is_undefined(handler) {
                return f64::from_bits(super::super::TAG_FALSE);
            }
            if is_callable_value(handler) {
                let handler = scope.root_nanbox_f64(handler);
                dispatch_listener(stream_h, event_h, arg_handles, &handler, false);
                return f64::from_bits(super::super::TAG_TRUE);
            }
        }
    }
    if is_error {
        if let Some(first) = arg_handles.with_live(|args| args.first().copied()) {
            let first = scope.root_nanbox_f64(first);
            let key = super::super::hidden_error_key();
            super::super::set_hidden_value(stream_h.get_nanbox_f64(), key, first.get_nanbox_f64());
            super::super::refresh_readable_aborted_flag(stream_h.get_nanbox_f64());
        }
    }
    let mut events = events_of(stream_h.get_nanbox_f64());
    let mut unhandled_error = is_error;
    match events {
        Some(found) => {
            if is_error {
                let found = scope.root_nanbox_f64(found);
                let monitor = error_monitor_event();
                if !is_undefined(get_key(found.get_nanbox_f64(), monitor)) {
                    let mut monitor_args = Vec::new();
                    monitor_args.push(monitor);
                    arg_handles.with_live(|live| monitor_args.extend_from_slice(live));
                    emit_via_method(stream_h.get_nanbox_f64(), &monitor_args);
                }
                unhandled_error = is_undefined(get_event_named(
                    found.get_nanbox_f64(),
                    crate::runtime_state_key!(b"error"),
                ));
                // A monitor listener may have replaced `_events`.
                events = events_of(stream_h.get_nanbox_f64());
            }
        }
        None if !is_error => return f64::from_bits(super::super::TAG_FALSE),
        None => {}
    }
    if unhandled_error {
        let live = arg_handles.with_live(<[f64]>::to_vec);
        if emit_unhandled_error_to_domain(stream_h.get_nanbox_f64(), live.first().copied()) {
            return f64::from_bits(super::super::TAG_FALSE);
        }
        crate::os::os_process_emitter::throw_unhandled_error_event(&live);
    }

    let Some(events) = events else {
        return f64::from_bits(super::super::TAG_FALSE);
    };
    let handler = get_key(events, event_h.get_nanbox_f64());
    if is_undefined(handler) {
        return f64::from_bits(super::super::TAG_FALSE);
    }
    if is_callable_value(handler) {
        // One listener: nothing to snapshot.
        let handler = scope.root_nanbox_f64(handler);
        dispatch_listener(stream_h, event_h, arg_handles, &handler, is_error);
        return f64::from_bits(super::super::TAG_TRUE);
    }
    // node clones the array before dispatch, so listeners added or removed
    // by a listener take effect from the next emit.
    let listener_values: Vec<f64> = if is_array_value(handler) {
        let arr = super::super::raw_ptr_from_value(handler) as *const crate::array::ArrayHeader;
        (0..crate::array::js_array_length(arr))
            .map(|i| crate::array::js_array_get_f64(arr, i))
            .collect()
    } else {
        Vec::new()
    };
    let listener_handles = scope.root_nanbox_f64_slice(&listener_values);
    for handle in &listener_handles {
        dispatch_listener(stream_h, event_h, arg_handles, handle, is_error);
    }
    f64::from_bits(super::super::TAG_TRUE)
}

/// One listener call of an `emit`, and node's `addCatch` for a listener that
/// returned a promise.
fn dispatch_listener(
    stream_h: &crate::gc::RuntimeHandle<'_>,
    event_h: &crate::gc::RuntimeHandle<'_>,
    arg_handles: &RootedArgs<'_>,
    listener: &crate::gc::RuntimeHandle<'_>,
    is_error: bool,
) {
    let result = arg_handles.with_live(|live| {
        call_listener_args(stream_h.get_nanbox_f64(), listener.get_nanbox_f64(), live)
    });
    if crate::promise::js_value_is_promise(result) == 0 {
        return;
    }
    // Node's Readable data delivery path does not route async `data` listener
    // rejections through captureRejections; custom EventEmitter-style events do.
    // As in node's `addCatch`, the capture flag is consulted only for a
    // listener that returned a promise.
    let is_data = super::super::string_value_eq(event_h.get_nanbox_f64(), b"data");
    if !is_error && !is_data && capture_rejections_enabled(stream_h.get_nanbox_f64()) {
        arg_handles.with_live(|live| {
            capture_listener_rejection(
                stream_h.get_nanbox_f64(),
                event_h.get_nanbox_f64(),
                live,
                result,
            )
        });
    } else if is_data {
        // Node's Readable swallows a rejection returned by an async `data`
        // listener — it is neither captured to `error` nor surfaced as an
        // unhandled rejection. Mark it handled so it stays silent (#1545).
        swallow_listener_rejection(result);
    }
}

/// node's `domain` module: an emitter bound to a domain (`domain.add(ee)`, an
/// own `domain` property) hands an unhandled `'error'` to that domain instead
/// of throwing: `er.domainEmitter = this`, `er.domain = domain`,
/// `er.domainThrown = false`, then `domain.emit('error', er)`.
fn emit_unhandled_error_to_domain(target: f64, error: Option<f64>) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let domain = get_named(
        target.get_nanbox_f64(),
        crate::runtime_state_key!(b"domain"),
    );
    let domain_value = JSValue::from_bits(domain.to_bits());
    if domain_value.is_undefined() || domain_value.is_null() || !domain_value.is_pointer() {
        return false;
    }
    let domain = scope.root_nanbox_f64(domain);
    let error = scope.root_nanbox_f64(error.unwrap_or_else(undefined_value));
    // `typeof er === 'object'`: an Error or any other object takes the notes.
    if JSValue::from_bits(error.get_nanbox_u64()).is_pointer()
        && unsafe { crate::symbol::js_is_symbol(error.get_nanbox_f64()) } == 0
    {
        for name in [&b"domainEmitter"[..], b"domain", b"domainThrown"] {
            let key = scope.root_nanbox_f64(name_key(name));
            // Read after the key's allocation: the handles track any move.
            let value = match name {
                b"domainEmitter" => target.get_nanbox_f64(),
                b"domain" => domain.get_nanbox_f64(),
                _ => bool_value(false),
            };
            unsafe {
                crate::object::js_object_set_property_key(
                    error.get_nanbox_f64(),
                    key.get_nanbox_f64(),
                    value,
                );
            }
        }
    }
    let args = [name_key(b"error"), error.get_nanbox_f64()];
    let method = crate::runtime_state_key!(b"emit");
    unsafe {
        method.call_value(domain.get_nanbox_f64(), args.as_ptr(), args.len());
    }
    true
}

fn error_monitor_event() -> f64 {
    unsafe { crate::symbol::js_symbol_for(super::super::literal_string_value(b"events.errorMonitor")) }
}
