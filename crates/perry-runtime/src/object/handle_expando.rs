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
//! GC: handle ids are stable small integers that never move, so — unlike the
//! closure table — no metadata re-keying is needed. Only the stored VALUES are
//! real JS references, so the registered mutable root scanner traces them in
//! every phase (keeping e.g. a stored array and its elements alive) and rewrites
//! the stored bits when a copying collection moves the value.

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
thread_local! {
    static HANDLE_EXPANDO_PROPS: RefCell<HashMap<i64, HashMap<String, u64>>> =
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
        cell.borrow_mut()
            .entry(handle)
            .or_default()
            .insert(name.to_string(), bits);
    });
    // Parent is the (non-heap) handle id, so pass 0 as the parent address — the
    // scanner traces the value unconditionally, and the barrier only needs to
    // mark the freshly stored child for an in-progress collection.
    crate::gc::runtime_write_barrier_external_slot(0, 0, bits);
}

/// Read back an own property previously stored via `handle_expando_set`.
/// Returns `None` when no such property exists (caller falls through to its
/// `undefined` default). Mirrors `closure_get_own_dynamic_prop`.
pub fn handle_expando_get(handle: i64, name: &str) -> Option<f64> {
    if handle == 0 {
        return None;
    }
    HANDLE_EXPANDO_PROPS
        .with(|cell| {
            cell.borrow()
                .get(&handle)
                .and_then(|p| p.get(name).copied())
        })
        .map(f64::from_bits)
}

/// True when the handle has at least one user-assigned expando property.
/// (`Object.keys` / `in` support can build on this later.)
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
        for bits in props.values_mut() {
            let mut v = f64::from_bits(*bits);
            visitor.visit_nanbox_f64_slot(&mut v);
            *bits = v.to_bits();
        }
        HANDLE_EXPANDO_PROPS.with(|cell| {
            match cell.borrow_mut().entry(owner) {
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    // A re-entrant set added/updated entries while we held no
                    // borrow; those newer writes must win. Only restore scanned
                    // keys that were not concurrently re-written.
                    let dst = e.get_mut();
                    for (k, v) in props {
                        dst.entry(k).or_insert(v);
                    }
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
}
