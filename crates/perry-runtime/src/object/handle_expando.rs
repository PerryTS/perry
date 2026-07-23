//! Generic per-handle expando property side-table.
//!
//! A native HANDLE value (Blob / fetch Response / Web-Streams reader / etc.) is
//! a NaN-boxed small integer id, NOT a heap `ObjectHeader`. The object setter
//! (`js_object_set_field_by_name`) routes a `handle.prop = v` write to
//! `js_handle_property_set_dispatch`, and a read to `js_handle_property_dispatch`.
//! Those dispatchers only know specific, typed properties (`blob.size`,
//! `response.status`, …). An ARBITRARY user-assigned own property
//! (`handle.colors = [...]`) had nowhere to land — the write was dropped and the
//! read returned `undefined`.
//!
//! In Node these objects are ordinary and freely extensible (the `debug`
//! package assigns `createDebug.colors = [...]` and later reads it back). This
//! side-table gives every handle the same arbitrary string-keyed own-property
//! storage that closures get from `CLOSURE_PROPS` (see
//! `closure/dynamic_props.rs`), modeled directly on that code.
//!
//! Attributes / accessors (#6363): `Object.defineProperty(handle, k, desc)`
//! stores the VALUE here but records the `writable`/`enumerable`/`configurable`
//! bits and any `get`/`set` pair in the ordinary
//! `descriptor_state::{PROPERTY_DESCRIPTORS, ACCESSOR_DESCRIPTORS}` side tables,
//! keyed by the handle id. Those tables are keyed by a plain `usize` and heap
//! addresses always sit above `HANDLE_BAND_MAX`, so a handle id can never
//! collide with a real owner address; their GC scanners leave the key alone
//! (a handle is not a forwardable heap address) and their dead-owner sweep
//! skips it (`attributed_owner_header` rejects an address that belongs to no
//! arena page / malloc header). Reusing them gets attribute + accessor storage,
//! rooting of the accessor closures, and `getOwnPropertyDescriptor` for free.
//!
//! GC: handle ids are stable small integers that never move, so — unlike the
//! closure table — no metadata re-keying is needed. Only the stored VALUES are
//! real JS references, so the registered mutable root scanner traces them in
//! every phase (keeping e.g. a stored array and its elements alive) and rewrites
//! the stored bits when a copying collection moves the value.

use super::descriptor_state::{get_accessor_descriptor, get_property_attrs, PropertyAttrs};
use std::cell::RefCell;
use std::collections::HashMap;

// Per-thread storage: each runtime thread has its own arena + GC, and the
// stored values are NaN-boxed references into THAT thread's arena. A
// process-global table would let one thread's GC scanner trace/rewrite another
// thread's values across arena boundaries (cross-thread values are deep-copied,
// so a handle id never legitimately escapes its owning thread). Thread-local
// keeps the side-table aligned with the per-thread GC, matching the documented
// threading model. The mutable root scanner is registered once but reads the
// CURRENT thread's table on each GC, so each thread traces only its own values.
//
// The per-handle entry is a `Vec<(name, bits)>` rather than a `HashMap` so own
// keys come back in INSERTION order — `Object.keys(handle)` / `{...handle}` are
// ordered in JS, and handles carry a handful of expandos at most, so the linear
// scan is cheaper than hashing.
thread_local! {
    static HANDLE_EXPANDO_PROPS: RefCell<HashMap<i64, Vec<(String, u64)>>> =
        RefCell::new(HashMap::new());
}

/// Store an arbitrary own property `name = value` on the handle `handle`.
/// Mirrors `closure_set_dynamic_prop`. The value is kept alive by the GC
/// scanner below; a write barrier publishes it for incremental/young marking.
pub fn handle_expando_set(handle: i64, name: &str, value: f64) {
    if handle == 0 {
        return;
    }
    let bits = value.to_bits();
    HANDLE_EXPANDO_PROPS.with(|cell| {
        let mut map = cell.borrow_mut();
        let props = map.entry(handle).or_default();
        match props.iter_mut().find(|(k, _)| k == name) {
            Some(slot) => slot.1 = bits,
            None => props.push((name.to_string(), bits)),
        }
    });
    // Parent is the (non-heap) handle id, so pass 0 as the parent address — the
    // scanner traces the value unconditionally, and the barrier only needs to
    // mark the freshly stored child for an in-progress collection.
    crate::gc::runtime_write_barrier_external_slot(0, 0, bits);
}

/// Drop every expando property stored under `handle`.
///
/// #6710: handle ids are recycled through perry-ffi's freelist — a freed
/// `IncomingMessage`/`ServerResponse`/Blob/stream id is handed back out by a
/// later `register_handle`. This table (like the symbol-property table) is
/// keyed by that id, so without an explicit clear the NEW owner inherits the
/// PREVIOUS owner's arbitrary JS props. Under concurrent HTTP requests that
/// crosses per-request state — Next.js stores `isRSCRequest` /
/// `NextInternalRequestMeta` on `req`, so a recycled id makes one request read
/// another's flags and the App Router render pipeline wedges. Callers clear a
/// recycled id's side tables on the MAIN (JS-owning) thread before reuse.
pub fn handle_expando_clear(handle: i64) {
    if handle == 0 {
        return;
    }
    HANDLE_EXPANDO_PROPS.with(|cell| {
        cell.borrow_mut().remove(&handle);
    });
    // Also drop the handle's property-attr / accessor descriptors, which live
    // in the generic per-owner descriptor tables (keyed by the handle id).
    super::descriptor_state::clear_object_descriptors(handle as usize);
}

/// Read back an own property previously stored via `handle_expando_set`.
/// Returns `None` when no such property exists (caller falls through to its
/// `undefined` default). Mirrors `closure_get_own_dynamic_prop`.
///
/// #6363: an own property installed by `Object.defineProperty(handle, k, {get})`
/// has no stored value — the getter must run instead. This is the single choke
/// point every handle read funnels through (perry-stdlib's
/// `js_handle_property_dispatch` consults it after every typed property misses),
/// so resolving the accessor here makes `handle.accessorProp` work on every read
/// path at once.
pub fn handle_expando_get(handle: i64, name: &str) -> Option<f64> {
    if handle == 0 {
        return None;
    }
    if let Some(acc) = handle_expando_accessor(handle, name) {
        if acc.get == 0 {
            // Setter-only accessor: reads yield `undefined`, they do NOT fall
            // through to a stale data value.
            return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
        }
        let closure =
            (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
        if closure.is_null() {
            return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
        }
        return Some(crate::closure::js_closure_call0(closure));
    }
    handle_expando_data_get(handle, name)
}

/// The stored DATA value for `(handle, name)`, ignoring any accessor. Used by
/// `getOwnPropertyDescriptor` (which must report the descriptor without running
/// the getter) and by the accessor-aware [`handle_expando_get`] above.
pub(crate) fn handle_expando_data_get(handle: i64, name: &str) -> Option<f64> {
    if handle == 0 {
        return None;
    }
    HANDLE_EXPANDO_PROPS
        .with(|cell| {
            cell.borrow()
                .get(&handle)
                .and_then(|p| p.iter().find(|(k, _)| k == name).map(|(_, v)| *v))
        })
        .map(f64::from_bits)
}

/// The accessor pair installed on `(handle, name)` by a `defineProperty`
/// accessor descriptor, if any.
pub(crate) fn handle_expando_accessor(
    handle: i64,
    name: &str,
) -> Option<super::descriptor_state::AccessorDescriptor> {
    if handle == 0 {
        return None;
    }
    get_accessor_descriptor(handle as usize, name)
}

/// The attributes of the own expando `(handle, name)`. A plain
/// `handle.foo = v` write records no entry, so it defaults — like any ordinary
/// JS assignment — to `{writable, enumerable, configurable}: true`.
pub(crate) fn handle_expando_attrs(handle: i64, name: &str) -> PropertyAttrs {
    get_property_attrs(handle as usize, name).unwrap_or(PropertyAttrs::new(true, true, true))
}

/// True when `name` is an own expando property of `handle` (data OR accessor).
pub(crate) fn handle_expando_has(handle: i64, name: &str) -> bool {
    if handle == 0 {
        return false;
    }
    if handle_expando_accessor(handle, name).is_some() {
        return true;
    }
    HANDLE_EXPANDO_PROPS.with(|cell| {
        cell.borrow()
            .get(&handle)
            .map(|p| p.iter().any(|(k, _)| k == name))
            .unwrap_or(false)
    })
}

/// Own expando property names of `handle`, in insertion order. When
/// `enumerable_only`, non-enumerable entries (a `defineProperty` default) are
/// filtered out — that is the `Object.keys` / `for-in` / spread surface;
/// `Object.getOwnPropertyNames` passes `false`.
pub(crate) fn handle_expando_own_keys(handle: i64, enumerable_only: bool) -> Vec<String> {
    if handle == 0 {
        return Vec::new();
    }
    let mut keys: Vec<String> = HANDLE_EXPANDO_PROPS.with(|cell| {
        cell.borrow()
            .get(&handle)
            .map(|p| p.iter().map(|(k, _)| k.clone()).collect())
            .unwrap_or_default()
    });
    // A pure accessor define stores no data slot, so pick those up from the
    // accessor table (appended after the data keys — close enough to insertion
    // order for the mixed case, and exact for the common all-data one).
    for k in super::descriptor_state::accessor_descriptor_keys_for_obj(handle as usize) {
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    if enumerable_only {
        keys.retain(|k| handle_expando_attrs(handle, k).enumerable());
    }
    keys
}

/// Remove the own expando `(handle, name)` and its descriptor state. Returns
/// `false` when the property exists but is non-configurable (`[[Delete]]`
/// rejects it), `true` otherwise — the same contract as an ordinary object's
/// `[[Delete]]`, so `delete handle.absent` still reports `true`.
pub(crate) fn handle_expando_delete(handle: i64, name: &str) -> bool {
    if handle == 0 {
        return true;
    }
    if !handle_expando_has(handle, name) {
        return true;
    }
    if !handle_expando_attrs(handle, name).configurable() {
        return false;
    }
    HANDLE_EXPANDO_PROPS.with(|cell| {
        if let Some(props) = cell.borrow_mut().get_mut(&handle) {
            props.retain(|(k, _)| k != name);
        }
    });
    let st = crate::state::state();
    st.descriptors
        .property_descriptors
        .borrow_mut()
        .remove(&(handle as usize, name.to_string()));
    st.descriptors
        .accessor_descriptors
        .borrow_mut()
        .remove(&(handle as usize, name.to_string()));
    true
}

/// True when the handle has at least one user-assigned expando property.
#[allow(dead_code)]
pub fn handle_expando_has_any(handle: i64) -> bool {
    if handle == 0 {
        return false;
    }
    HANDLE_EXPANDO_PROPS.with(|cell| {
        cell.borrow()
            .get(&handle)
            .map(|p| !p.is_empty())
            .unwrap_or(false)
    })
}

/// Mutable GC root scanner for the handle expando side-table.
///
/// Keys are stable small handle ids (never heap-moved), so this only traces the
/// stored VALUES — exactly the value half of
/// `scan_closure_dynamic_props_roots_mut`. Registered in `gc/mod.rs`. The
/// per-owner entry is removed (borrow dropped) before invoking the visitor on
/// each value, because the visitor may move objects and re-enter the runtime
/// (e.g. a `handle_expando_set` on this same thread) — matching the closure
/// scanner's contract.
pub fn scan_handle_expando_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let owners: Vec<i64> =
        HANDLE_EXPANDO_PROPS.with(|cell| cell.borrow().keys().copied().collect());
    for owner in owners {
        let Some(mut props) = HANDLE_EXPANDO_PROPS.with(|cell| cell.borrow_mut().remove(&owner))
        else {
            continue;
        };
        for (_, bits) in props.iter_mut() {
            let mut v = f64::from_bits(*bits);
            visitor.visit_nanbox_f64_slot(&mut v);
            *bits = v.to_bits();
        }
        HANDLE_EXPANDO_PROPS.with(|cell| {
            match cell.borrow_mut().entry(owner) {
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    // A re-entrant set added/updated entries while we held no
                    // borrow; those newer writes must win. Only restore scanned
                    // keys that were not concurrently re-written, and keep the
                    // scanned (older) keys FIRST so insertion order survives.
                    let dst = e.get_mut();
                    for (k, v) in props.iter_mut() {
                        if let Some(newer) = dst.iter().find(|(nk, _)| nk == k) {
                            *v = newer.1;
                        }
                    }
                    for (k, v) in dst.iter() {
                        if !props.iter().any(|(pk, _)| pk == k) {
                            props.push((k.clone(), *v));
                        }
                    }
                    // GC_STORE_AUDIT(ROOT): HANDLE_EXPANDO_PROPS entries are scanned by
                    // scan_handle_expando_roots_mut (this function) — the merged vec holds
                    // values this pass just traced/rewrote, plus any re-entrant write that
                    // the mutator already barriered on its own way in.
                    *dst = props;
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(props);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_roundtrip() {
        let h = 0x4_2424i64;
        assert!(handle_expando_get(h, "colors").is_none());
        let v = f64::from_bits(0x7FFD_AAAA_BBBB_CCCC);
        handle_expando_set(h, "colors", v);
        assert_eq!(
            handle_expando_get(h, "colors").map(|x| x.to_bits()),
            Some(v.to_bits())
        );
        assert!(handle_expando_has_any(h));
        // cleanup
        HANDLE_EXPANDO_PROPS.with(|cell| {
            cell.borrow_mut().remove(&h);
        });
    }

    #[test]
    fn clear_drops_all_props() {
        // #6710: a recycled handle id must not carry the prior owner's props.
        let h = 0x4_2426i64;
        handle_expando_set(h, "a", f64::from_bits(0x7FFD_0000_0000_0011));
        handle_expando_set(h, "b", f64::from_bits(0x7FFD_0000_0000_0022));
        assert!(handle_expando_has_any(h));
        handle_expando_clear(h);
        assert!(
            !handle_expando_has_any(h),
            "clear must drop the owner entry"
        );
        assert!(handle_expando_get(h, "a").is_none());
        assert!(handle_expando_get(h, "b").is_none());
        // Clearing an absent id and the null id (0) are no-ops, not panics.
        handle_expando_clear(0x9_9999i64);
        handle_expando_clear(0);
    }

    #[test]
    fn clear_drops_handle_descriptors() {
        // #6710: a handle that received a `defineProperty` descriptor must also
        // have it cleared on recycle — exercises the HANDLE_HAS_DESCRIPTORS-gated
        // path in `clear_object_descriptors`.
        use super::super::descriptor_state::{
            get_property_attrs, set_property_attrs, PropertyAttrs,
        };
        let h = 0x4_2427i64;
        set_property_attrs(
            h as usize,
            "d".to_string(),
            PropertyAttrs::new(false, true, true),
        );
        assert!(get_property_attrs(h as usize, "d").is_some());
        handle_expando_clear(h);
        assert!(
            get_property_attrs(h as usize, "d").is_none(),
            "clear must drop the handle's property descriptor"
        );
    }

    #[test]
    fn scanner_visits_stored_values() {
        let h = 0x4_2425i64;
        let v_bits = 0x7FFD_1234_5678_9ABCu64;
        handle_expando_set(h, "x", f64::from_bits(v_bits));
        let mut seen: Vec<u64> = Vec::new();
        {
            let mut mark = |v: f64| seen.push(v.to_bits());
            let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(&mut mark);
            scan_handle_expando_roots_mut(&mut visitor);
        }
        assert!(
            seen.contains(&v_bits),
            "scanner must trace stored value, seen={seen:x?}"
        );
        HANDLE_EXPANDO_PROPS.with(|cell| {
            cell.borrow_mut().remove(&h);
        });
    }

    /// #6363: own keys come back in INSERTION order (`Object.keys(handle)` /
    /// `{...handle}` are ordered in JS), and a re-set of an existing key must
    /// update in place rather than move it to the back.
    #[test]
    fn own_keys_preserve_insertion_order() {
        let h = 0x4_2426i64;
        for name in ["b", "a", "c"] {
            handle_expando_set(h, name, f64::from_bits(crate::value::TAG_TRUE));
        }
        handle_expando_set(h, "b", f64::from_bits(crate::value::TAG_FALSE));
        assert_eq!(handle_expando_own_keys(h, false), vec!["b", "a", "c"]);
        HANDLE_EXPANDO_PROPS.with(|cell| {
            cell.borrow_mut().remove(&h);
        });
    }

    /// #6363: a `defineProperty` default descriptor is NON-enumerable, so it
    /// stays out of the `Object.keys` surface but is still an own property —
    /// and being non-configurable, `[[Delete]]` must reject it.
    #[test]
    fn non_enumerable_define_hides_from_keys_and_rejects_delete() {
        let h = 0x4_2427i64;
        handle_expando_set(h, "vis", f64::from_bits(crate::value::TAG_TRUE));
        handle_expando_set(h, "hid", f64::from_bits(crate::value::TAG_TRUE));
        super::super::descriptor_state::set_property_attrs(
            h as usize,
            "hid".to_string(),
            PropertyAttrs::new(false, false, false),
        );

        assert!(handle_expando_has(h, "hid"));
        assert_eq!(handle_expando_own_keys(h, false), vec!["vis", "hid"]);
        assert_eq!(handle_expando_own_keys(h, true), vec!["vis"]);

        // Non-configurable → delete rejects, property survives.
        assert!(!handle_expando_delete(h, "hid"));
        assert!(handle_expando_has(h, "hid"));
        // Absent key → delete reports success (Node: `delete x.__nope` is true).
        assert!(handle_expando_delete(h, "__nope"));
        // Plain-write default is fully configurable → delete removes it.
        assert!(handle_expando_delete(h, "vis"));
        assert!(!handle_expando_has(h, "vis"));

        HANDLE_EXPANDO_PROPS.with(|cell| {
            cell.borrow_mut().remove(&h);
        });
        crate::state::state()
            .descriptors
            .property_descriptors
            .borrow_mut()
            .remove(&(h as usize, "hid".to_string()));
    }
}
