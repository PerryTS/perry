//! A `node:dgram` socket's private state record.
//!
//! The runtime's bookkeeping for a socket (its type, bind and connect state,
//! addresses, buffer sizes, lookup, send block list, abort wiring, reactor
//! id, async ids) lives in one fixed record of NaN-boxed words, found from
//! the socket object itself: its `meta.native_state` names the record, as a
//! stream's names its state record (`node_stream::state_record`). A slot is
//! a fixed index, so a read is a few dependent loads and a write the array's
//! store with its barrier: no key text, no atom lookup, no own-key scan, no
//! prototype walk, nothing an own property of the socket (node keeps this
//! state behind a private symbol, so `Object.keys(socket)` shows none of it
//! and nothing on `Object.prototype` stands in for it).
//!
//! An absent slot reads `undefined`, exactly as the old absent hidden key
//! did. Word 0 holds the record's brand, so a record is never mistaken for
//! another family's array (a stream's state record is told by its length).
//! The meta edge is traced and rewritten by the meta descriptor
//! (`gc/layout_slot_visit.rs`, any pointer-tagged `native_state`); the record
//! is reachable only through its socket. No side table, address map or
//! registry.

use crate::array::ArrayHeader;
use crate::object::ObjectHeader;

/// One word of a socket's state record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub(crate) enum Slot {
    /// The record's brand ([`BRAND_BITS`]); never a state datum.
    Brand,
    Type,
    Bound,
    Closed,
    Address,
    Family,
    Port,
    Connected,
    RemoteAddress,
    RemoteFamily,
    RemotePort,
    RecvBufferSize,
    SendBufferSize,
    ReuseAddr,
    Ipv6Only,
    Lookup,
    SendBlockList,
    BindAttempted,
    AbortSignal,
    AbortListener,
    /// Reactor id for the live OS socket (real mode only); links a JS socket
    /// back to its `UdpSocket` + recv thread in [`crate::dgram_reactor`].
    ReactorId,
    AsyncId,
    TriggerAsyncId,
}

const SLOT_COUNT: usize = Slot::TriggerAsyncId as usize + 1;

/// Word 0 of every socket record: a plain double no state datum stores
/// there (the slot is never written after the birth).
const BRAND_BITS: u64 = 0x4133_4911_0000_4911;

/// `socket`'s record, when it has one. Never allocates.
#[inline]
fn record_of(socket: f64) -> Option<*mut ArrayHeader> {
    let obj = crate::object::field_get_set::runtime_read_site::object_receiver(socket)?;
    // SAFETY: a live GC_TYPE_OBJECT (object_receiver checked the header).
    let meta = unsafe {
        crate::meta_read!(crate::meta_obj_from_type!(
            obj as *mut ObjectHeader,
            crate::gc::GC_TYPE_OBJECT
        ))
    };
    if meta.is_null() {
        return None;
    }
    // SAFETY: the meta record of a live object.
    record_in_word(unsafe { (*meta).native_state })
}

#[inline]
fn record_in_word(word: u64) -> Option<*mut ArrayHeader> {
    if !crate::native_payload::is_payload_state_word(word) {
        return None;
    }
    let record = (word & crate::value::POINTER_MASK) as *mut ArrayHeader;
    // SAFETY: a pointer-tagged `native_state` word is a traced edge to a live
    // GC cell; the type check precedes the array reads.
    unsafe {
        (crate::dgram::gc_type_for_ptr(record as usize) == Some(crate::gc::GC_TYPE_ARRAY)
            && (*record).length as usize == SLOT_COUNT
            && *crate::array::array_elements_ptr(record) == BRAND_BITS)
            .then_some(record)
    }
}

/// The value in `slot`, `None` when absent (`undefined`) or when `socket`
/// has no record. A direct load behind the record check: no property read,
/// no call that can collect or reenter.
#[inline]
pub(crate) fn read(socket: f64, slot: Slot) -> Option<f64> {
    let record = record_of(socket)?;
    // SAFETY: `slot` indexes the record's fixed span.
    let bits = unsafe { *crate::array::array_elements_ptr(record).add(slot as usize) };
    (bits != crate::value::TAG_UNDEFINED).then(|| f64::from_bits(bits))
}

/// Store `value` in `slot`, giving `socket` a record first if it has none
/// (a socket gets one at its birth, so only a borrowed receiver lands here).
/// An object whose `native_state` belongs to another family keeps it; the
/// store is dropped there, as a store to a non-object always was.
pub(crate) fn write(socket: f64, slot: Slot, value: f64) {
    debug_assert!(slot != Slot::Brand);
    if let Some(record) = record_of(socket) {
        store(record, slot as usize, value);
        return;
    }
    if value.to_bits() == crate::value::TAG_UNDEFINED {
        // Absent already.
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let socket = scope.root_nanbox_f64(socket);
    let value = scope.root_nanbox_f64(value);
    ensure(socket.get_nanbox_f64());
    if let Some(record) = record_of(socket.get_nanbox_f64()) {
        store(record, slot as usize, value.get_nanbox_f64());
    }
}

#[inline(never)]
fn store(record: *mut ArrayHeader, slot: usize, value: f64) {
    // The array store: string aliasing, layout note and write barrier.
    crate::string::js_string_addref_if_heap_string(value);
    // SAFETY: record_of proved this runtime-owned array of the fixed span;
    // no safepoint between record_of and the header flag read.
    unsafe {
        let flags = (*crate::gc::header_from_trusted_user_ptr(record.cast()))._reserved;
        crate::array::store_array_slot_resolved(record, slot, value, flags);
    };
}

/// Give `socket` its record: [`SLOT_COUNT`] words, the brand then every
/// datum absent, held in its own `native_state`. An object whose
/// `native_state` already names something gets none.
pub(crate) fn ensure(socket: f64) -> bool {
    if record_of(socket).is_some() {
        return true;
    }
    let Some(obj) = crate::object::field_get_set::runtime_read_site::object_receiver(socket) else {
        return false;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let meta = obj.with_mut_ptr::<ObjectHeader, _>(|o| unsafe {
        crate::object::object_meta_ensure(
            crate::meta_obj_from_type!(o, crate::gc::GC_TYPE_OBJECT),
            crate::object::MetaReason::RuntimeHandle,
        )
    });
    if meta.is_null() || unsafe { (*meta).native_state } != 0 {
        return false;
    }
    let record = crate::array::js_array_alloc_with_length_exact(SLOT_COUNT as u32);
    // SAFETY: a fresh array of exactly that length; scalars need no barrier
    // on a newborn array.
    unsafe {
        let elements = crate::array::array_elements_ptr(record);
        // GC_STORE_AUDIT(POINTER_FREE): the brand, a plain double.
        elements.write(BRAND_BITS);
        for i in 1..SLOT_COUNT {
            // GC_STORE_AUDIT(POINTER_FREE): the absent marker.
            elements.add(i).write(crate::value::TAG_UNDEFINED);
        }
    }
    let record_word = crate::value::JSValue::pointer(record as *const u8).bits();
    obj.with_mut_ptr::<ObjectHeader, _>(|o| unsafe {
        // The allocation above may have moved the object: re-read its meta.
        let meta = crate::meta_read!(crate::meta_obj_from_type!(o, crate::gc::GC_TYPE_OBJECT));
        (*meta).native_state = record_word;
        crate::gc::runtime_write_barrier_slot(
            meta as usize,
            &(*meta).native_state as *const _ as usize,
            record_word,
        );
    });
    true
}

#[cfg(test)]
mod tests {
    use crate::dgram::{
        get_hidden_value, hidden_string, object_ptr_from_value, set_hidden_value, socket_object,
        str_value, string_to_rust, KEY_ADDRESS, KEY_CONNECTED, KEY_SEND_BLOCK_LIST, KEY_TYPE,
    };
    use crate::object::{js_object_get_field_by_name_f64, ObjectHeader};
    use crate::value::{TAG_TRUE, TAG_UNDEFINED};

    fn key(name: &str) -> *mut crate::string::StringHeader {
        crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
    }

    fn own_key_names(value: f64) -> Vec<String> {
        let obj = object_ptr_from_value(value).expect("an object");
        let keys = crate::object::js_object_keys(obj as *const ObjectHeader);
        (0..crate::array::js_array_length(keys))
            .filter_map(|i| string_to_rust(crate::array::js_array_get_f64(keys, i)))
            .collect()
    }

    /// node keeps a socket's state behind a private symbol: no own key of the
    /// socket names it, and the runtime reads it from the socket's record.
    #[test]
    fn socket_state_is_its_record_not_own_keys() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let socket = scope.root_nanbox_f64(socket_object("udp6"));
        let s = socket.get_nanbox_f64();
        assert_eq!(hidden_string(s, KEY_TYPE).as_deref(), Some("udp6"));
        let names = own_key_names(s);
        assert!(
            names.iter().any(|n| n == "send"),
            "methods stay own keys: {names:?}"
        );
        assert!(
            names.iter().all(|n| !n.starts_with("__perry")),
            "no runtime state among the own keys: {names:?}"
        );
        let obj = object_ptr_from_value(s).unwrap();
        assert_eq!(
            js_object_get_field_by_name_f64(obj, key("__perryDgramType")).to_bits(),
            TAG_UNDEFINED,
            "the state is not a named property"
        );
        assert!(
            get_hidden_value(s, KEY_SEND_BLOCK_LIST).is_none(),
            "absent reads None"
        );
    }

    /// A property of the same text is never the socket's state: an object
    /// without a record reads every slot absent, whatever it (or its
    /// prototype chain) holds under the old key names.
    #[test]
    fn a_named_property_never_stands_in_for_state() {
        let obj = crate::object::js_object_alloc(0, 1);
        crate::object::js_object_set_field_by_name(
            obj,
            key("__perryDgramConnected"),
            f64::from_bits(TAG_TRUE),
        );
        let value = crate::value::js_nanbox_pointer(obj as i64);
        assert!(get_hidden_value(value, KEY_CONNECTED).is_none());
    }

    /// The record is reached only through its socket: a moving collection
    /// keeps the values it names alive and rewrites the edges. The scope's
    /// roots are seen by a collection only once the runtime-handle scanner
    /// is registered (without it the full collection frees the socket itself
    /// and the test fails for the wrong reason).
    #[test]
    fn record_values_survive_collections() {
        let _lock = crate::gc::global_side_table_test_lock();
        crate::gc::register_runtime_handle_root_scanner_for_tests();
        let scope = crate::gc::RuntimeHandleScope::new();
        let socket = scope.root_nanbox_f64(socket_object("udp4"));
        let text = "10.20.30.40-a-young-heap-string-for-the-record";
        set_hidden_value(socket.get_nanbox_f64(), KEY_ADDRESS, str_value(text));
        crate::gc::gc_collect_minor();
        for i in 0..4000 {
            let s = format!("garbage-garbage-garbage-{i:08}");
            let _ = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        }
        crate::gc::gc_collect_minor();
        crate::gc::js_gc_collect();
        assert_eq!(
            hidden_string(socket.get_nanbox_f64(), KEY_ADDRESS).as_deref(),
            Some(text)
        );
        assert_eq!(
            hidden_string(socket.get_nanbox_f64(), KEY_TYPE).as_deref(),
            Some("udp4")
        );
    }
}
