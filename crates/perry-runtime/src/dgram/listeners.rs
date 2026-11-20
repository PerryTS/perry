//! `node:dgram` listener storage, add/remove/emit, and `eventNames()`.
//!
//! A socket's listeners live in its state record (`record.rs`,
//! [`Slot::Events`](super::Slot::Events)): one [[Prototype]]-null ordinary
//! object, as node's `_events`, whose keys are the event names' property
//! atoms. A key's value is one array of `(listener, once)` pairs, replaced
//! whole by every add and remove and never written in place, so an emit runs
//! over the array it read (its snapshot): a listener added or removed by a
//! listener changes the next emit only. A once listener leaves the list just
//! before it runs, as node's once wrapper removes itself. An event whose last listener goes is
//! deleted from the object, as node deletes `_events[type]`, so
//! `eventNames()` is the object's own keys in order.
//!
//! An emit names its event by the agent's atom for the text: a lookup, never
//! an allocation; a text with no atom is no key of any events object (an add
//! mints the atom it keys by), so such an emit has no listener. The pair array
//! is read through the events object's layout probe
//! ([`runtime_read_site::get_atom`]): the object's shape answers each read,
//! and nothing (no slot index, no memo) is remembered between emits. No key
//! text is built and nothing is an own property of the socket.

use super::*;

use crate::array::ArrayHeader;
use crate::gc::RuntimeHandleScope;
use crate::object::field_get_set::runtime_read_site;
use crate::object::{js_object_keys, js_object_set_field_by_name, ObjectHeader};
use crate::string::StringHeader;
use crate::value::TAG_UNDEFINED;

/// The property atom of the event name `event` when its text has one.
/// Never allocates. `None` also for a non-string event.
fn event_atom(event: f64) -> Option<*const StringHeader> {
    // SAFETY: `event` is a live value.
    unsafe { crate::string::existing_property_atom_of_value(event.to_bits()) }
}

/// The property atom of a runtime-fixed event name, when the text has one.
/// Never allocates.
fn name_atom(name: &str) -> Option<*const StringHeader> {
    let bytes = name.as_bytes();
    let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
    // SAFETY: the table's entries are live strings.
    crate::string::atom_lookup(bytes, hash)
        .filter(|atom| unsafe { crate::string::is_property_atom(*atom) })
}

fn atom_value(atom: *const StringHeader) -> f64 {
    f64::from_bits(JSValue::string_ptr(atom as *mut StringHeader).bits())
}

fn atom_of_value(value: f64) -> *const StringHeader {
    (value.to_bits() & crate::value::POINTER_MASK) as *const StringHeader
}

/// `socket`'s events object, when it has one.
fn events_of(socket: f64) -> Option<*mut ObjectHeader> {
    runtime_read_site::object_receiver(get_hidden_value(socket, KEY_EVENTS)?)
}

/// The `(listener, once)` pair array `socket` holds for the event `atom`.
fn pairs_of(socket: f64, atom: *const StringHeader) -> Option<f64> {
    let events = events_of(socket)?;
    // SAFETY: a live ordinary object (`object_receiver`) and a live property
    // atom; the events object holds only data properties.
    let value = unsafe { runtime_read_site::get_atom(events, atom) };
    (value.to_bits() != TAG_UNDEFINED && raw_ptr_from_value(value) != 0).then_some(value)
}

fn pairs_len(pairs: f64) -> u32 {
    crate::array::js_array_length(raw_ptr_from_value(pairs) as *const ArrayHeader)
}

/// Make `pairs` the listeners of the event `atom` (a string value), or,
/// when `pairs` is `undefined`, delete the event: it has none left.
fn set_pairs(socket: f64, atom: f64, pairs: f64) {
    let scope = RuntimeHandleScope::new();
    let socket = scope.root_nanbox_f64(socket);
    let atom = scope.root_nanbox_f64(atom);
    let pairs = scope.root_nanbox_f64(pairs);
    let none_left = pairs.get_nanbox_f64().to_bits() == TAG_UNDEFINED;
    if events_of(socket.get_nanbox_f64()).is_none() {
        if none_left {
            return;
        }
        let events = crate::object::js_object_alloc_null_proto_ordinary(2);
        set_hidden_value(
            socket.get_nanbox_f64(),
            KEY_EVENTS,
            boxed_pointer(events as *const u8),
        );
    }
    let Some(events) = events_of(socket.get_nanbox_f64()) else {
        return;
    };
    let key = atom_of_value(atom.get_nanbox_f64());
    if none_left {
        crate::object::js_object_delete_field(events, key);
    } else {
        js_object_set_field_by_name(events, key, pairs.get_nanbox_f64());
    }
}

pub(crate) fn add_listener(socket: f64, event: f64, listener: f64, once: bool) {
    if string_to_rust(event).is_none() {
        return;
    }
    if !is_callable_value(listener) {
        throw_invalid_listener(listener);
    }
    let scope = RuntimeHandleScope::new();
    let socket = scope.root_nanbox_f64(socket);
    let listener = scope.root_nanbox_f64(listener);
    // SAFETY: `event` is a live string value.
    let atom = scope.root_nanbox_f64(f64::from_bits(unsafe {
        crate::string::property_atom_from_value(event.to_bits())
    }));
    let old = scope.root_nanbox_f64(
        pairs_of(
            socket.get_nanbox_f64(),
            atom_of_value(atom.get_nanbox_f64()),
        )
        .unwrap_or_else(undefined_value),
    );
    let len = if old.get_nanbox_f64().to_bits() == TAG_UNDEFINED {
        0
    } else {
        pairs_len(old.get_nanbox_f64())
    };
    let mut out = crate::array::js_array_alloc(len + 2);
    let old_raw = raw_ptr_from_value(old.get_nanbox_f64()) as *const ArrayHeader;
    for i in 0..len {
        out = crate::array::js_array_push_f64(out, crate::array::js_array_get_f64(old_raw, i));
    }
    out = crate::array::js_array_push_f64(out, listener.get_nanbox_f64());
    out = crate::array::js_array_push_f64(out, bool_value(once));
    set_pairs(
        socket.get_nanbox_f64(),
        atom.get_nanbox_f64(),
        boxed_pointer(out as *const u8),
    );
}

/// `pairs` without the pairs `drop(pair index)` names, stored as the
/// event's listeners (the event deleted when none is left).
fn replace_pairs(socket: f64, atom: *const StringHeader, pairs: f64, drop: impl Fn(u32) -> bool) {
    let scope = RuntimeHandleScope::new();
    let socket = scope.root_nanbox_f64(socket);
    let atom = scope.root_nanbox_f64(atom_value(atom));
    let pairs = scope.root_nanbox_f64(pairs);
    let len = pairs_len(pairs.get_nanbox_f64());
    let kept = (0..len / 2).filter(|&p| !drop(p)).count() as u32;
    if kept == 0 {
        set_pairs(
            socket.get_nanbox_f64(),
            atom.get_nanbox_f64(),
            undefined_value(),
        );
        return;
    }
    let mut out = crate::array::js_array_alloc(kept * 2);
    let raw = raw_ptr_from_value(pairs.get_nanbox_f64()) as *const ArrayHeader;
    for p in (0..len / 2).filter(|&p| !drop(p)) {
        for i in [2 * p, 2 * p + 1] {
            out = crate::array::js_array_push_f64(out, crate::array::js_array_get_f64(raw, i));
        }
    }
    set_pairs(
        socket.get_nanbox_f64(),
        atom.get_nanbox_f64(),
        boxed_pointer(out as *const u8),
    );
}

pub(crate) fn listener_snapshot(socket: f64, event: f64) -> Vec<(f64, bool)> {
    let Some(pairs) = event_atom(event).and_then(|atom| pairs_of(socket, atom)) else {
        return Vec::new();
    };
    let raw = raw_ptr_from_value(pairs) as *const ArrayHeader;
    (0..pairs_len(pairs) / 2)
        .map(|p| {
            (
                crate::array::js_array_get_f64(raw, 2 * p),
                crate::value::js_is_truthy(crate::array::js_array_get_f64(raw, 2 * p + 1)) != 0,
            )
        })
        .collect()
}

pub(crate) fn remove_listener(socket: f64, event: f64, listener: f64) -> bool {
    let Some(atom) = event_atom(event) else {
        return false;
    };
    let Some(pairs) = pairs_of(socket, atom) else {
        return false;
    };
    let raw = raw_ptr_from_value(pairs) as *const ArrayHeader;
    let Some(remove) = (0..pairs_len(pairs) / 2)
        .rev()
        .find(|&p| crate::array::js_array_get_f64(raw, 2 * p).to_bits() == listener.to_bits())
    else {
        return false;
    };
    replace_pairs(socket, atom, pairs, |p| p == remove);
    true
}

pub(crate) fn emit_event_value(socket: f64, event: f64, args: &[f64]) -> bool {
    match event_atom(event) {
        Some(atom) => emit_atom(socket, atom, args),
        None => false,
    }
}

pub(crate) fn emit_event(socket: f64, event: &str, args: &[f64]) -> bool {
    match name_atom(event) {
        Some(atom) => emit_atom(socket, atom, args),
        None => false,
    }
}

fn emit_atom(socket: f64, atom: *const StringHeader, args: &[f64]) -> bool {
    let Some(pairs) = pairs_of(socket, atom) else {
        return false;
    };
    let len = pairs_len(pairs);
    if len == 0 {
        return false;
    }
    let scope = RuntimeHandleScope::new();
    let socket = scope.root_nanbox_f64(socket);
    let atom = scope.root_nanbox_f64(atom_value(atom));
    let pairs = scope.root_nanbox_f64(pairs);
    let async_ids = socket_async_ids(socket.get_nanbox_f64());
    crate::async_hook_slots::enter_resource_scope(async_ids);
    for p in 0..len / 2 {
        let raw = raw_ptr_from_value(pairs.get_nanbox_f64()) as *const ArrayHeader;
        let listener = crate::array::js_array_get_f64(raw, 2 * p);
        if crate::value::js_is_truthy(crate::array::js_array_get_f64(raw, 2 * p + 1)) != 0 {
            // node's once wrapper: the listener leaves the event's list
            // just before it runs (a no-op when a listener removed it).
            remove_once_pair(
                socket.get_nanbox_f64(),
                atom_of_value(atom.get_nanbox_f64()),
                listener,
            );
        }
        call_function(listener, socket.get_nanbox_f64(), args);
    }
    crate::async_hook_slots::leave_resource_scope(async_ids.async_id);
    true
}

/// Remove the last once pair of `listener` from the event's current list.
fn remove_once_pair(socket: f64, atom: *const StringHeader, listener: f64) {
    let Some(pairs) = pairs_of(socket, atom) else {
        return;
    };
    let raw = raw_ptr_from_value(pairs) as *const ArrayHeader;
    let found = (0..pairs_len(pairs) / 2).rev().find(|&p| {
        crate::array::js_array_get_f64(raw, 2 * p).to_bits() == listener.to_bits()
            && crate::value::js_is_truthy(crate::array::js_array_get_f64(raw, 2 * p + 1)) != 0
    });
    if let Some(remove) = found {
        replace_pairs(socket, atom, pairs, |p| p == remove);
    }
}

/// `socket.eventNames()`: the events with at least one listener, in the
/// order node's `Reflect.ownKeys(_events)` gives (an event is deleted when
/// its last listener goes, so a re-added event comes last).
pub(crate) fn event_names_impl(socket: f64) -> f64 {
    match events_of(socket) {
        Some(events) => boxed_pointer(js_object_keys(events) as *const u8),
        None => boxed_pointer(crate::array::js_array_alloc(0) as *const u8),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::closure::{ClosureHeader, JsThis};
    use std::cell::RefCell;

    thread_local! {
        static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn log() -> Vec<String> {
        LOG.with(|l| l.borrow_mut().drain(..).collect())
    }

    /// A listener that logs `tag` (capture 0, a string) and `message`'s
    /// listener count, then removes capture 1 and adds capture 2 (when set).
    extern "C" fn body(c: *const ClosureHeader, this: JsThis, _msg: f64, _rinfo: f64) -> f64 {
        let socket = this.as_f64();
        let tag = string_to_rust(crate::closure::js_closure_get_capture_f64(c, 0)).unwrap();
        let count = listener_snapshot(socket, str_value("message")).len();
        LOG.with(|l| l.borrow_mut().push(format!("{tag}:{count}")));
        let remove = crate::closure::js_closure_get_capture_f64(c, 1);
        if remove.to_bits() != TAG_UNDEFINED {
            remove_listener(socket, str_value("message"), remove);
        }
        let add = crate::closure::js_closure_get_capture_f64(c, 2);
        if add.to_bits() != TAG_UNDEFINED {
            add_listener(socket, str_value("message"), add, false);
        }
        undefined_value()
    }

    fn listener(tag: &str, remove: f64, add: f64) -> f64 {
        let c = crate::closure::js_closure_alloc(crate::fn_info!(body, 2), 3);
        crate::closure::js_closure_set_capture_f64(c, 0, str_value(tag));
        crate::closure::js_closure_set_capture_f64(c, 1, remove);
        crate::closure::js_closure_set_capture_f64(c, 2, add);
        crate::value::js_nanbox_pointer(c as i64)
    }

    fn emit(socket: f64) -> bool {
        emit_event(socket, "message", &[undefined_value(), undefined_value()])
    }

    fn names(socket: f64) -> Vec<String> {
        let arr = raw_ptr_from_value(event_names_impl(socket)) as *const ArrayHeader;
        (0..crate::array::js_array_length(arr))
            .filter_map(|i| string_to_rust(crate::array::js_array_get_f64(arr, i)))
            .collect()
    }

    /// As node: a once listener runs in the first emit only and leaves the
    /// list just before it runs (an earlier listener still counts it); a
    /// listener removed by an earlier one still runs in the running emit
    /// (its snapshot) and not in the next; a listener added during an emit
    /// runs from the next emit on.
    #[test]
    fn an_emit_runs_its_snapshot_and_once_listeners_once() {
        let _stable = crate::gc::GcSuppressScope::new();
        let socket = socket_object("udp4");
        let u = undefined_value();
        let late = listener("late", u, u);
        let victim = listener("victim", u, u);
        let first = listener("first", victim, late);
        add_listener(socket, str_value("message"), first, false);
        add_listener(socket, str_value("message"), listener("once", u, u), true);
        add_listener(socket, str_value("message"), victim, false);
        assert!(emit(socket));
        // `first` sees `once` still listed, removes `victim` and adds
        // `late`; `once` leaves as it runs; `victim` runs from the snapshot.
        assert_eq!(log(), ["first:3", "once:2", "victim:2"]);
        assert!(emit(socket));
        // `first` removes `victim` again (absent) and adds a second `late`.
        assert_eq!(log(), ["first:2", "late:3"]);
        assert_eq!(listener_snapshot(socket, str_value("message")).len(), 3);
    }

    /// The listeners are the record's, not own keys of the socket; an event
    /// whose last listener goes is deleted, so a re-added event comes last
    /// in `eventNames()` (node deletes `_events[type]`); an emit of a name
    /// no listener ever had finds nothing.
    #[test]
    fn listeners_are_the_records_and_names_follow_node() {
        let _stable = crate::gc::GcSuppressScope::new();
        let socket = socket_object("udp4");
        let u = undefined_value();
        let a = listener("a", u, u);
        add_listener(socket, str_value("close"), a, false);
        add_listener(socket, str_value("message"), listener("m", u, u), false);
        assert_eq!(names(socket), ["close", "message"]);
        let obj = object_ptr_from_value(socket).unwrap();
        let keys = js_object_keys(obj as *const ObjectHeader);
        for i in 0..crate::array::js_array_length(keys) {
            let k = string_to_rust(crate::array::js_array_get_f64(keys, i)).unwrap();
            assert!(
                !k.starts_with("__perry"),
                "listener storage is an own key: {k}"
            );
        }
        assert!(remove_listener(socket, str_value("close"), a));
        assert!(!remove_listener(socket, str_value("close"), a));
        assert_eq!(names(socket), ["message"]);
        add_listener(socket, str_value("close"), a, false);
        assert_eq!(names(socket), ["message", "close"]);
        assert!(!emit_event(socket, "wtf29-never-a-key", &[]));
        assert!(emit(socket));
        assert_eq!(log(), ["m:1"]);
    }
}
