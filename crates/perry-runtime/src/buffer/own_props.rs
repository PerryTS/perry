//! Own (dynamic) properties assigned onto a `Buffer` / typed-array value.
//!
//! Perry allocates buffers as raw `BufferHeader`s outside the object model, so
//! a plain `buf.foo = v` had nowhere to go: the set was dropped and the read
//! returned `undefined`. Node's Buffer is a `Uint8Array` — an ordinary object —
//! so user code freely stores properties on one, and it also *shadows* the
//! prototype's methods when the key collides.
//!
//! mysql2 sizes every outgoing packet with exactly that idiom
//! (`packets/packet.js` → `MockBuffer`):
//!
//! ```js
//! const noop = function () {};
//! const mock = Buffer.alloc(0);
//! for (const k in Packet.prototype)
//!     if (typeof mock[k] === "function") mock[k] = noop;   // neutralize writes
//! // …serialize once against `mock` to MEASURE, then for real against
//! // Buffer.alloc(mock.offset)
//! ```
//!
//! Without own-prop storage the no-ops never landed, the measuring pass wrote
//! into the zero-length Buffer, and the MySQL handshake died with
//! `RangeError [ERR_OUT_OF_RANGE]`.
//!
//! Mirrors `closure::dynamic_props` (same locked side table + GC root scanner
//! contract): values are traced in EVERY phase so a stored closure/array stays
//! reachable, and the owner key is rewritten on evacuation.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

type BufferProps = HashMap<usize, HashMap<String, u64>>;

fn buffer_props() -> &'static Mutex<BufferProps> {
    static PROPS: OnceLock<Mutex<BufferProps>> = OnceLock::new();
    PROPS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Monotonic "some buffer own prop was ever stored" flag (#6386). Hot
/// accessor fast paths (DataView get*/set*) use it to skip the mutex +
/// double-HashMap shadow probe entirely in the overwhelmingly common program
/// that never assigns properties onto a buffer/typed-array/DataView. Set
/// (release) BEFORE the table insert, so a `false` (acquire) read guarantees
/// no insert has completed — the probe it skips could only have found nothing.
static BUFFER_OWN_PROPS_EVER: AtomicBool = AtomicBool::new(false);

/// `false` while no buffer own prop has ever been stored process-wide.
pub fn buffer_own_props_possible() -> bool {
    BUFFER_OWN_PROPS_EVER.load(Ordering::Acquire)
}

/// Store `buf.<prop> = value`. Only reached for a registered buffer address.
pub fn buffer_set_own_prop(addr: usize, prop: &str, value: f64) {
    if addr == 0 {
        return;
    }
    if let Some(accessor) = crate::object::get_accessor_descriptor(addr, prop) {
        if accessor.set != 0 {
            unsafe {
                crate::object::invoke_accessor_setter(
                    accessor.set,
                    crate::value::js_nanbox_pointer(addr as i64),
                    value,
                );
            }
        }
        return;
    }
    if crate::object::get_property_attrs(addr, prop).is_some_and(|attrs| !attrs.writable())
        && buffer_get_own_prop(addr, prop).is_some()
    {
        return;
    }
    buffer_define_own_data_prop(addr, prop, value);
}

/// Descriptor installation bypasses ordinary [[Set]] interception after it
/// has validated the redefinition and selected the new property kind.
pub fn buffer_define_own_data_prop(addr: usize, prop: &str, value: f64) {
    if addr == 0 {
        return;
    }
    BUFFER_OWN_PROPS_EVER.store(true, Ordering::Release);
    if let Ok(mut props) = buffer_props().lock() {
        props
            .entry(addr)
            .or_default()
            .insert(prop.to_string(), value.to_bits());
    }
}

/// Read an own dynamic prop, or `None` when the buffer has no such key.
pub fn buffer_get_own_prop(addr: usize, prop: &str) -> Option<f64> {
    if addr == 0 {
        return None;
    }
    buffer_props()
        .lock()
        .ok()
        .and_then(|props| props.get(&addr).and_then(|m| m.get(prop)).copied())
        .map(f64::from_bits)
}

/// Every own dynamic prop key recorded for `addr`, in insertion-independent
/// (sorted) order.
///
/// #8149: `Object.keys` / `getOwnPropertyNames` / `for…in` need these. Before,
/// the enumeration paths had no registered-buffer arm at all and walked a
/// `BufferHeader` as an `ObjectHeader` — reading payload bytes as the
/// `keys_array` pointer, which returned `[]` when those bytes happened to be
/// zero and SIGBUS'd in `js_array_length` when they did not.
///
/// Integer-index keys come back as the canonical decimal strings they were
/// stored under (`buffer::canonical_index_key`); the caller is responsible for
/// the ECMA-262 ordering rule that puts array indices first, ascending.
pub fn buffer_own_prop_names(addr: usize) -> Vec<String> {
    if addr == 0 || !buffer_own_props_possible() {
        return Vec::new();
    }
    let mut names: Vec<String> = buffer_props()
        .lock()
        .ok()
        .and_then(|props| props.get(&addr).map(|m| m.keys().cloned().collect()))
        .unwrap_or_default();
    names.sort();
    names
}

/// Whether the buffer carries any own dynamic prop under `prop`.
pub fn buffer_has_own_prop(addr: usize, prop: &str) -> bool {
    buffer_get_own_prop(addr, prop).is_some()
        || crate::object::get_accessor_descriptor(addr, prop).is_some()
}

/// Delete an ordinary named own property from a registered buffer/view.
/// Returns whether the property was present.
pub fn buffer_delete_own_prop(addr: usize, prop: &str) -> bool {
    if addr == 0 || !buffer_own_props_possible() {
        return false;
    }
    let Ok(mut props) = buffer_props().lock() else {
        return false;
    };
    let Some(entries) = props.get_mut(&addr) else {
        return false;
    };
    let removed = entries.remove(prop).is_some();
    crate::object::clear_accessor_descriptor(addr, prop);
    crate::object::clear_property_attrs(addr, prop);
    if entries.is_empty() {
        props.remove(&addr);
    }
    removed
}

/// GC: trace stored values in every phase (a stored closure is reachable ONLY
/// through this table) and rewrite the owner address when the buffer moves.
pub fn scan_buffer_own_props_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let owners = buffer_props()
        .lock()
        .ok()
        .map(|props| props.keys().copied().collect::<Vec<_>>())
        .unwrap_or_default();
    for owner in owners {
        let Some(mut entries) = buffer_props()
            .lock()
            .ok()
            .and_then(|mut props| props.remove(&owner))
        else {
            continue;
        };
        let mut new_owner = owner;
        visitor.visit_metadata_usize_slot(&mut new_owner);
        for bits in entries.values_mut() {
            let mut v = f64::from_bits(*bits);
            visitor.visit_nanbox_f64_slot(&mut v);
            *bits = v.to_bits();
        }
        if let Ok(mut props) = buffer_props().lock() {
            props.insert(new_owner, entries);
        }
    }
}

/// Live owner count. Test-only: the leak regression asserts the table DRAINS
/// after the owning buffers are collected, which a per-address
/// `buffer_get_own_prop` probe cannot show.
#[cfg(test)]
pub(crate) fn test_buffer_own_props_owner_count() -> usize {
    buffer_props().lock().map(|props| props.len()).unwrap_or(0)
}

/// Drop every own prop recorded for `addr`.
///
/// Two callers, and they cover different halves of the address-keyed table's
/// lifetime:
///
/// * `register_buffer` — a *recycled* address must not inherit the previous
///   tenant's expandos.
/// * `finalize_collected_dead_buffer` — the owning buffer DIED. Registration
///   alone is not enough: it only fires if the address is re-issued to another
///   *buffer*, so an entry whose address is never reused, or is reused by a
///   plain object, used to survive for the life of the process. That leaked one
///   entry per property-carrying Buffer/DataView ever created, and
///   `scan_buffer_own_props_roots_mut` kept tracing the stored values in every
///   GC phase — so a dead buffer's expando closure stayed reachable forever, and
///   its dead owner key kept being re-resolved against whatever now occupies
///   that address.
pub fn clear_buffer_own_props(addr: usize) {
    if addr == 0 {
        return;
    }
    if let Ok(mut props) = buffer_props().lock() {
        props.remove(&addr);
    }
}
