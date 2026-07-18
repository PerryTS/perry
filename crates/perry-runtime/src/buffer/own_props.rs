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

/// Whether the buffer carries any own dynamic prop under `prop`.
pub fn buffer_has_own_prop(addr: usize, prop: &str) -> bool {
    buffer_get_own_prop(addr, prop).is_some()
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

/// Drop every own prop recorded for `addr`. Called when a buffer is registered
/// (freed storage is recycled, and the table is address-keyed).
pub fn clear_buffer_own_props(addr: usize) {
    if addr == 0 {
        return;
    }
    if let Ok(mut props) = buffer_props().lock() {
        props.remove(&addr);
    }
}
