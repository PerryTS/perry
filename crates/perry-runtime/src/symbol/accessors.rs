use std::collections::HashMap;
use std::sync::Mutex;

use super::{
    obj_key_from_f64, publish_symbol_side_table_root_edges, sym_key_from_f64, SYMBOL_PROPERTIES,
    TAG_UNDEFINED,
};

#[derive(Clone, Copy)]
pub(super) struct SymbolAccessorDescriptor {
    pub(super) get: u64,
    pub(super) set: u64,
}

per_test_global! {
    static SYMBOL_ACCESSOR_PROPERTIES: Mutex<
        Option<HashMap<(usize, usize), SymbolAccessorDescriptor>>,
    > = Mutex::new(None);
}

pub(super) fn clear_symbol_accessor_property(obj_key: usize, sym_key: usize) {
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_ACCESSOR_PROPERTIES);
    if let Some(map) = guard.as_mut() {
        map.remove(&(obj_key, sym_key));
    }
}

/// #6710: drop every symbol accessor descriptor owned by `obj_key` — used when
/// a handle id is recycled so a reused id can't carry the prior owner's
/// accessors. Keyed by `(obj_key, sym_key)`, so retain everything else.
pub(super) fn clear_all_symbol_accessor_properties_for_object(obj_key: usize) {
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_ACCESSOR_PROPERTIES);
    if let Some(map) = guard.as_mut() {
        map.retain(|(o, _), _| *o != obj_key);
    }
}

/// #8195: death pruning for `SYMBOL_ACCESSOR_PROPERTIES`, called from
/// `symbol::prune_dead_symbol_property_owners` so all three symbol tables
/// share one deadness verdict per owner.
///
/// The `sym_key` half of the key is a strong root
/// (`scan_symbol_accessor_roots_mut`'s `visit_usize_slot`); the OWNER half is
/// metadata-only and rekeyed, so it can go stale. Until now the only bulk
/// removal was `clear_all_symbol_accessor_properties_for_object`, reached
/// solely from the handle-recycle path — never for a plain heap object. Two
/// consequences, both closed here: the accessor closures held in the
/// descriptor were immortal, and the dead owner address survived into the next
/// cycle's rewrite pass (#8040's shape; see `gc::dead_owner`).
pub(super) fn prune_dead_symbol_accessor_owners(is_dead_owner: &dyn Fn(usize) -> bool) {
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_ACCESSOR_PROPERTIES);
    if let Some(map) = guard.as_mut() {
        map.retain(|(owner, _), _| !is_dead_owner(*owner));
    }
}

#[cfg(test)]
pub(crate) fn test_symbol_accessor_property_count() -> usize {
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_ACCESSOR_PROPERTIES);
    guard.as_ref().map_or(0, |map| map.len())
}

#[cfg(test)]
pub(crate) fn test_seed_symbol_accessor_property(obj_key: usize, sym_key: usize, get_bits: u64) {
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_ACCESSOR_PROPERTIES);
    guard.get_or_insert_with(HashMap::new).insert(
        (obj_key, sym_key),
        SymbolAccessorDescriptor {
            get: get_bits,
            set: TAG_UNDEFINED,
        },
    );
}

pub(crate) unsafe fn set_symbol_accessor_property(
    obj_f64: f64,
    sym_f64: f64,
    get_bits: u64,
    set_bits: u64,
) {
    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return;
    }
    crate::symbol::note_symbol_key_installed(sym_key);
    {
        // `SYMBOL_PROPERTIES` is the only insertion-ordered record of symbol
        // property CREATION order, which `[[OwnPropertyKeys]]` must report
        // (test262 getOwnPropertySymbols/order-after-define-property).
        // Removing the data entry on a data→accessor redefine — or never
        // adding one for a fresh accessor install — destroys that position,
        // so the key re-enumerated at the end (or in creation-id order, which
        // is not install order). Keep an order-preserving placeholder instead:
        // same key, TAG_UNDEFINED value bits so the old data value stops
        // being rooted. Readers never mistake it for a data value — get, set,
        // gOPD and has-own all consult `SYMBOL_ACCESSOR_PROPERTIES` first,
        // and `clone_symbol_entries_for_obj_ptr` filters accessor-keyed
        // entries out for the raw-entry consumers (formatting, freeze/seal).
        let mut props = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
        if props.is_none() {
            *props = Some(HashMap::new());
        }
        let entries = props.as_mut().unwrap().entry(obj_key).or_default();
        if let Some(entry) = entries.iter_mut().find(|entry| entry.0 == sym_key) {
            entry.1 = crate::value::TAG_UNDEFINED;
        } else {
            entries.push((sym_key, crate::value::TAG_UNDEFINED));
        }
    }
    {
        let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_ACCESSOR_PROPERTIES);
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        guard.as_mut().unwrap().insert(
            (obj_key, sym_key),
            SymbolAccessorDescriptor {
                get: get_bits,
                set: set_bits,
            },
        );
    }
    if get_bits != 0 {
        publish_symbol_side_table_root_edges(sym_key, get_bits);
    }
    if set_bits != 0 {
        publish_symbol_side_table_root_edges(sym_key, set_bits);
    }
}

pub(super) unsafe fn symbol_accessor_property(
    obj_f64: f64,
    sym_f64: f64,
) -> Option<SymbolAccessorDescriptor> {
    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return None;
    }
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_ACCESSOR_PROPERTIES);
    guard
        .as_ref()
        .and_then(|m| m.get(&(obj_key, sym_key)).copied())
}

pub(super) fn symbol_accessor_property_by_key(
    obj_key: usize,
    sym_key: usize,
) -> Option<SymbolAccessorDescriptor> {
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_ACCESSOR_PROPERTIES);
    guard
        .as_ref()
        .and_then(|m| m.get(&(obj_key, sym_key)).copied())
}

pub(super) unsafe fn invoke_symbol_accessor_getter(get_bits: u64, receiver: f64) -> f64 {
    if get_bits == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let closure = (get_bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let prev = crate::object::js_implicit_this_set(receiver);
    let result = crate::closure::js_closure_call0(closure);
    crate::object::js_implicit_this_set(prev);
    result
}

pub(super) fn scan_symbol_accessor_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut rewrites = Vec::new();
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_ACCESSOR_PROPERTIES);
    let Some(map) = guard.as_mut() else {
        return;
    };

    for (old_owner, old_sym_key) in map.keys().copied().collect::<Vec<_>>() {
        let Some(acc) = map.get_mut(&(old_owner, old_sym_key)) else {
            continue;
        };
        let mut new_owner = old_owner;
        let mut new_sym_key = old_sym_key;
        let owner_changed =
            visitor.visit_metadata_usize_slot(&mut new_owner) && new_owner != old_owner;
        let sym_changed = visitor.visit_usize_slot(&mut new_sym_key) && new_sym_key != old_sym_key;
        if acc.get != 0 {
            visitor.visit_nanbox_u64_slot(&mut acc.get);
        }
        if acc.set != 0 {
            visitor.visit_nanbox_u64_slot(&mut acc.set);
        }
        if owner_changed || sym_changed {
            rewrites.push(((old_owner, old_sym_key), (new_owner, new_sym_key)));
        }
    }

    for (old_key, new_key) in rewrites {
        if let Some(acc) = map.remove(&old_key) {
            map.insert(new_key, acc);
        }
    }
}

/// Snapshot of the accessor table's keys for the budgeted step scanner.
pub(super) fn accessor_property_keys() -> Vec<(usize, usize)> {
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_ACCESSOR_PROPERTIES);
    guard
        .as_ref()
        .map(|m| m.keys().copied().collect())
        .unwrap_or_default()
}

/// Step twin of `scan_symbol_accessor_roots_mut` for one snapshot key:
/// strong-visits the get/set closures and rekeys owner/sym on a move.
/// Cycle-based collections run ONLY the step scanner, so before this
/// existed, symbol-keyed getter/setter closures reachable solely through
/// this table were swept by every full/fallback collection.
pub(super) fn scan_symbol_accessor_root_slot(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    owner: usize,
    sym_key: usize,
) {
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_ACCESSOR_PROPERTIES);
    let Some(map) = guard.as_mut() else {
        return;
    };
    let Some(acc) = map.get_mut(&(owner, sym_key)) else {
        return;
    };
    let mut new_owner = owner;
    let mut new_sym_key = sym_key;
    let owner_changed = visitor.visit_metadata_usize_slot(&mut new_owner) && new_owner != owner;
    let sym_changed = visitor.visit_usize_slot(&mut new_sym_key) && new_sym_key != sym_key;
    if acc.get != 0 {
        visitor.visit_nanbox_u64_slot(&mut acc.get);
    }
    if acc.set != 0 {
        visitor.visit_nanbox_u64_slot(&mut acc.set);
    }
    if owner_changed || sym_changed {
        if let Some(acc) = map.remove(&(owner, sym_key)) {
            map.insert((new_owner, new_sym_key), acc);
        }
    }
}

pub(super) fn has_own_symbol_accessor(obj_key: usize, sym_key: usize) -> bool {
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_ACCESSOR_PROPERTIES);
    guard
        .as_ref()
        .is_some_and(|m| m.contains_key(&(obj_key, sym_key)))
}

/// Symbol keys (raw `SymbolHeader` pointers) of every accessor-only property
/// installed on `obj_key`. Used by `getOwnPropertySymbols`, which must report
/// symbol-keyed accessors even though they live outside `SYMBOL_PROPERTIES`.
pub(super) fn owner_symbol_accessor_keys(obj_key: usize) -> Vec<usize> {
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_ACCESSOR_PROPERTIES);
    guard
        .as_ref()
        .map(|m| {
            m.keys()
                .filter(|(owner, _)| *owner == obj_key)
                .map(|(_, sym_key)| *sym_key)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
pub(super) fn test_clear_symbol_accessor_roots() {
    *crate::gc::lock_gc_root_registry(&SYMBOL_ACCESSOR_PROPERTIES) = None;
}
