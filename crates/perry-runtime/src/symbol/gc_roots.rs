//! GC root scanning + forwarding-rewrite for every symbol side table
//! (data properties, descriptor attrs, class-static symbols, the symbol
//! pointer metadata set), plus the incremental snapshot/step driver and the
//! `#[cfg(test)]` seed/inspect helpers.

use super::*;
use crate::string::{js_string_from_bytes, StringHeader};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

pub(crate) fn merge_symbol_property_entries(dst: &mut Vec<(usize, u64)>, src: Vec<(usize, u64)>) {
    for (sym_key, value_bits) in src {
        if let Some(existing) = dst.iter_mut().find(|entry| entry.0 == sym_key) {
            existing.1 = value_bits;
        } else {
            dst.push((sym_key, value_bits));
        }
    }
}

pub fn scan_symbol_side_table_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_symbol_side_table_roots_mut(&mut visitor);
}

pub fn scan_symbol_side_table_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    scan_symbol_property_roots_mut(visitor);
    scan_symbol_property_attrs_mut(visitor);
    accessors::scan_symbol_accessor_roots_mut(visitor);
    scan_class_static_symbol_roots_mut(visitor);
    scan_symbol_pointer_metadata_roots_mut(visitor);
}

fn scan_symbol_property_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut owner_rewrites = Vec::new();
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    let Some(map) = guard.as_mut() else {
        return;
    };

    for (&owner, entries) in map.iter_mut() {
        let mut new_owner = owner;
        if visitor.visit_metadata_usize_slot(&mut new_owner) && new_owner != owner {
            owner_rewrites.push((owner, new_owner));
        }
        for (sym_key, value_bits) in entries.iter_mut() {
            visitor.visit_usize_slot(sym_key);
            visitor.visit_nanbox_u64_slot(value_bits);
        }
    }

    for (old_owner, new_owner) in owner_rewrites {
        let Some(entries) = map.remove(&old_owner) else {
            continue;
        };
        match map.entry(new_owner) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                merge_symbol_property_entries(entry.get_mut(), entries);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(entries);
            }
        }
    }
}

fn scan_symbol_property_attrs_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut rewrites = Vec::new();
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTY_ATTRS);
    let Some(map) = guard.as_mut() else {
        return;
    };

    for (old_owner, old_sym_key) in map.keys().copied().collect::<Vec<_>>() {
        let mut new_owner = old_owner;
        let mut new_sym_key = old_sym_key;
        let owner_changed =
            visitor.visit_metadata_usize_slot(&mut new_owner) && new_owner != old_owner;
        let sym_changed = visitor.visit_usize_slot(&mut new_sym_key) && new_sym_key != old_sym_key;
        if owner_changed || sym_changed {
            rewrites.push(((old_owner, old_sym_key), (new_owner, new_sym_key)));
        }
    }

    for (old_key, new_key) in rewrites {
        if let Some(attrs) = map.remove(&old_key) {
            map.insert(new_key, attrs);
        }
    }
}

fn scan_class_static_symbol_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut key_rewrites = Vec::new();
    let mut guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
    let Some(map) = guard.as_mut() else {
        return;
    };

    for (class_id, old_sym_key) in map.keys().copied().collect::<Vec<_>>() {
        let Some(value_bits) = map.get_mut(&(class_id, old_sym_key)) else {
            continue;
        };
        let mut new_sym_key = old_sym_key;
        if visitor.visit_usize_slot(&mut new_sym_key) && new_sym_key != old_sym_key {
            key_rewrites.push(((class_id, old_sym_key), (class_id, new_sym_key)));
        }
        visitor.visit_nanbox_u64_slot(value_bits);
    }

    for (old_key, new_key) in key_rewrites {
        if let Some(value_bits) = map.remove(&old_key) {
            map.insert(new_key, value_bits);
        }
    }
}

fn scan_symbol_pointer_metadata_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut rewrites = Vec::new();
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS);
    let Some(set) = guard.as_mut() else {
        return;
    };
    for old_ptr in set.iter().copied().collect::<Vec<_>>() {
        let mut new_ptr = old_ptr;
        if visitor.visit_metadata_usize_slot(&mut new_ptr) && new_ptr != old_ptr {
            rewrites.push((old_ptr, new_ptr));
        }
    }
    for (old_ptr, new_ptr) in rewrites {
        set.remove(&old_ptr);
        if new_ptr != 0 {
            set.insert(new_ptr);
        }
    }
}

#[derive(Clone, Copy)]
enum SymbolSideTableRootSlot {
    SymbolPropertyOwner { owner: usize },
    SymbolPropertyEntry { owner: usize, sym_key: usize },
    SymbolPropertyAttrs { owner: usize, sym_key: usize },
    SymbolAccessorProperty { owner: usize, sym_key: usize },
    ClassStaticSymbol { class_id: u32, sym_key: usize },
    SymbolPointer { ptr: usize },
}

pub(crate) struct SymbolSideTableRootScanState {
    slots: Vec<SymbolSideTableRootSlot>,
    cursor: usize,
}

pub(crate) fn new_symbol_side_table_root_scan_state() -> Box<dyn std::any::Any> {
    Box::new(SymbolSideTableRootScanState {
        slots: symbol_side_table_root_snapshot(),
        cursor: 0,
    })
}

pub(crate) fn scan_symbol_side_table_roots_mut_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut dyn std::any::Any,
    remaining: &mut usize,
) -> bool {
    let state = state
        .downcast_mut::<SymbolSideTableRootScanState>()
        .expect("symbol side-table root scanner state type");
    while *remaining > 0 && state.cursor < state.slots.len() {
        scan_symbol_side_table_root_slot(visitor, state.slots[state.cursor]);
        state.cursor += 1;
        *remaining -= 1;
    }
    state.cursor >= state.slots.len()
}

fn symbol_side_table_root_snapshot() -> Vec<SymbolSideTableRootSlot> {
    let mut slots = Vec::new();

    {
        let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
        if let Some(map) = guard.as_ref() {
            for (&owner, entries) in map.iter() {
                slots.push(SymbolSideTableRootSlot::SymbolPropertyOwner { owner });
                for &(sym_key, _) in entries.iter() {
                    slots.push(SymbolSideTableRootSlot::SymbolPropertyEntry { owner, sym_key });
                }
            }
        }
    }

    {
        let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTY_ATTRS);
        if let Some(map) = guard.as_ref() {
            for &(owner, sym_key) in map.keys() {
                slots.push(SymbolSideTableRootSlot::SymbolPropertyAttrs { owner, sym_key });
            }
        }
    }

    for (owner, sym_key) in accessors::accessor_property_keys() {
        slots.push(SymbolSideTableRootSlot::SymbolAccessorProperty { owner, sym_key });
    }

    {
        let guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
        if let Some(map) = guard.as_ref() {
            for &(class_id, sym_key) in map.keys() {
                slots.push(SymbolSideTableRootSlot::ClassStaticSymbol { class_id, sym_key });
            }
        }
    }

    {
        let guard = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS);
        if let Some(set) = guard.as_ref() {
            for &ptr in set.iter() {
                slots.push(SymbolSideTableRootSlot::SymbolPointer { ptr });
            }
        }
    }

    slots
}

fn scan_symbol_side_table_root_slot(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    slot: SymbolSideTableRootSlot,
) {
    match slot {
        SymbolSideTableRootSlot::SymbolPropertyOwner { owner } => {
            rewrite_symbol_property_owner_if_forwarded(visitor, owner);
        }
        SymbolSideTableRootSlot::SymbolPropertyEntry { owner, sym_key } => {
            let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
            let Some((entry_sym, value_bits)) = guard
                .as_mut()
                .and_then(|map| map.get_mut(&owner))
                .and_then(|entries| entries.iter_mut().find(|entry| entry.0 == sym_key))
            else {
                return;
            };
            visitor.visit_usize_slot(entry_sym);
            visitor.visit_nanbox_u64_slot(value_bits);
        }
        SymbolSideTableRootSlot::SymbolAccessorProperty { owner, sym_key } => {
            accessors::scan_symbol_accessor_root_slot(visitor, owner, sym_key);
        }
        SymbolSideTableRootSlot::SymbolPropertyAttrs { owner, sym_key } => {
            rewrite_symbol_property_attrs_if_forwarded(visitor, owner, sym_key);
        }
        SymbolSideTableRootSlot::ClassStaticSymbol { class_id, sym_key } => {
            rewrite_class_static_symbol_entry_if_forwarded(visitor, class_id, sym_key);
        }
        SymbolSideTableRootSlot::SymbolPointer { ptr } => {
            rewrite_symbol_pointer_metadata_if_forwarded(visitor, ptr);
        }
    }
}

fn rewrite_symbol_property_owner_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    owner: usize,
) {
    let mut new_owner = owner;
    if !visitor.visit_metadata_usize_slot(&mut new_owner) || new_owner == owner {
        return;
    }
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    if let Some(map) = guard.as_mut() {
        if let Some(entries) = map.remove(&owner) {
            match map.entry(new_owner) {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    merge_symbol_property_entries(entry.get_mut(), entries);
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(entries);
                }
            }
        }
    }
}

fn rewrite_symbol_property_attrs_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    owner: usize,
    sym_key: usize,
) {
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTY_ATTRS);
    let Some(map) = guard.as_mut() else {
        return;
    };
    if !map.contains_key(&(owner, sym_key)) {
        return;
    }
    let mut new_owner = owner;
    let mut new_sym_key = sym_key;
    let owner_moved = visitor.visit_metadata_usize_slot(&mut new_owner);
    let sym_moved = visitor.visit_usize_slot(&mut new_sym_key);
    if (owner_moved && new_owner != owner) || (sym_moved && new_sym_key != sym_key) {
        if let Some(attrs) = map.remove(&(owner, sym_key)) {
            map.insert((new_owner, new_sym_key), attrs);
        }
    }
}

fn rewrite_class_static_symbol_entry_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    class_id: u32,
    sym_key: usize,
) {
    let mut guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
    let Some(map) = guard.as_mut() else {
        return;
    };
    let Some(value_bits) = map.get_mut(&(class_id, sym_key)) else {
        return;
    };
    let mut new_sym_key = sym_key;
    let moved = visitor.visit_usize_slot(&mut new_sym_key);
    visitor.visit_nanbox_u64_slot(value_bits);
    if moved && new_sym_key != sym_key {
        if let Some(value_bits) = map.remove(&(class_id, sym_key)) {
            map.insert((class_id, new_sym_key), value_bits);
        }
    }
}

fn rewrite_symbol_pointer_metadata_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    ptr: usize,
) {
    let mut new_ptr = ptr;
    if !visitor.visit_metadata_usize_slot(&mut new_ptr) || new_ptr == ptr {
        return;
    }
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS);
    if let Some(set) = guard.as_mut() {
        set.remove(&ptr);
        if new_ptr != 0 {
            set.insert(new_ptr);
        }
    }
}

#[cfg(test)]
pub(crate) fn test_clear_symbol_side_table_roots() {
    *crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES) = None;
    *crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTY_ATTRS) = None;
    *crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS) = None;
    accessors::test_clear_symbol_accessor_roots();

    let mut persistent = Vec::new();
    {
        let guard = SYMBOL_REGISTRY.lock().unwrap();
        if let Some(map) = guard.as_ref() {
            persistent.extend(map.values().copied());
        }
    }
    {
        let guard = WELL_KNOWN_SYMBOLS.lock().unwrap();
        if let Some(map) = guard.as_ref() {
            persistent.extend(map.values().copied());
        }
    }

    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS);
    if persistent.is_empty() {
        *guard = None;
    } else {
        *guard = Some(persistent.into_iter().collect());
    }
}

#[cfg(test)]
pub(crate) fn test_seed_symbol_property_root(owner: usize, sym_key: usize, value_bits: u64) {
    if owner != 0 && sym_key != 0 {
        store_object_symbol_property_root(owner, sym_key, value_bits);
    }
}

#[cfg(test)]
pub(crate) fn test_symbol_property_roots(owner: usize) -> Vec<(usize, u64)> {
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    guard
        .as_ref()
        .and_then(|map| map.get(&owner))
        .cloned()
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) fn test_symbol_property_root_bits(owner: usize, sym_key: usize) -> Option<u64> {
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    guard.as_ref().and_then(|map| {
        map.get(&owner)
            .and_then(|entries| entries.iter().find(|entry| entry.0 == sym_key))
            .map(|entry| entry.1)
    })
}

#[cfg(test)]
pub(crate) fn test_symbol_property_owner_exists(owner: usize) -> bool {
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    guard.as_ref().is_some_and(|map| map.contains_key(&owner))
}

#[cfg(test)]
pub(crate) fn test_seed_class_static_symbol_root(class_id: u32, sym_key: usize, value_bits: u64) {
    if class_id != 0 && sym_key != 0 {
        store_class_static_symbol_root(class_id, sym_key, value_bits);
    }
}

#[cfg(test)]
pub(crate) fn test_class_static_symbol_root_bits(class_id: u32, sym_key: usize) -> Option<u64> {
    let guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
    guard
        .as_ref()
        .and_then(|map| map.get(&(class_id, sym_key)).copied())
}

#[cfg(test)]
pub(crate) fn test_class_static_symbol_roots_for_class(class_id: u32) -> Vec<(usize, u64)> {
    let guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
    guard
        .as_ref()
        .map(|map| {
            map.iter()
                .filter_map(|(&(cid, sym_key), &value_bits)| {
                    (cid == class_id).then_some((sym_key, value_bits))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) fn test_seed_symbol_pointer_root(ptr: usize) {
    if ptr != 0 {
        register_symbol_pointer(ptr);
    }
}

#[cfg(test)]
pub(crate) fn test_symbol_pointer_root_contains(ptr: usize) -> bool {
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS);
    guard.as_ref().is_some_and(|set| set.contains(&ptr))
}
