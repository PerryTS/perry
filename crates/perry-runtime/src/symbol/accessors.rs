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

static SYMBOL_ACCESSOR_PROPERTIES: Mutex<
    Option<HashMap<(usize, usize), SymbolAccessorDescriptor>>,
> = Mutex::new(None);

pub(super) fn clear_symbol_accessor_property(obj_key: usize, sym_key: usize) {
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_ACCESSOR_PROPERTIES);
    if let Some(map) = guard.as_mut() {
        map.remove(&(obj_key, sym_key));
    }
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
        let mut props = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
        if let Some(map) = props.as_mut() {
            if let Some(entries) = map.get_mut(&obj_key) {
                entries.retain(|(key, _)| *key != sym_key);
            }
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
