//! GC root scanning + forwarding-rewrite for every symbol side table
//! (data properties, descriptor attrs, class-static symbols, the symbol
//! pointer metadata set), plus the incremental snapshot/step driver and the
//! `#[cfg(test)]` seed/inspect helpers.

use super::*;

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
    if visitor.young_scope() {
        scan_young_symbol_side_table_roots_mut(visitor);
        return;
    }
    scan_symbol_property_roots_mut(visitor);
    scan_symbol_property_attrs_mut(visitor);
    accessors::scan_symbol_accessor_roots_mut(visitor);
    scan_class_static_symbol_roots_mut(visitor);
    scan_symbol_pointer_metadata_roots_mut(visitor);
    rebuild_symbol_young_log();
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
            insert_symbol_pointer_in_set(set, new_ptr);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SymbolSideTableRootSlot {
    SymbolPropertyOwner { owner: usize },
    SymbolPropertyEntry { owner: usize, sym_key: usize },
    SymbolPropertyAttrs { owner: usize, sym_key: usize },
    SymbolAccessorProperty { owner: usize, sym_key: usize },
    ClassStaticSymbol { class_id: u32, sym_key: usize },
    SymbolPointer { ptr: usize },
}

crate::perry_thread_local! {
    static SYMBOL_SIDE_TABLE_YOUNG: RefCell<crate::gc::young_log::YoungLog<SymbolSideTableRootSlot>> =
        const { RefCell::new(crate::gc::young_log::YoungLog::new()) };
    #[cfg(test)]
    static TEST_SUPPRESS_SYMBOL_YOUNG_NOTE: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

const SYMBOL_YOUNG_LOG_NAME: &str = "symbol.side_tables";

#[inline]
fn note_symbol_slot(slot: SymbolSideTableRootSlot) {
    #[cfg(test)]
    if TEST_SUPPRESS_SYMBOL_YOUNG_NOTE.with(std::cell::Cell::get) {
        return;
    }
    SYMBOL_SIDE_TABLE_YOUNG.with(|log| log.borrow_mut().note(slot));
}

pub(super) fn note_symbol_property_root(owner: usize, sym_key: usize, value_bits: u64) {
    if crate::gc::young_log::addr_is_minor_collectible(owner) {
        note_symbol_slot(SymbolSideTableRootSlot::SymbolPropertyOwner { owner });
    }
    if crate::gc::young_log::addr_is_minor_relevant(sym_key)
        || crate::gc::young_log::bits_are_minor_relevant(value_bits)
    {
        note_symbol_slot(SymbolSideTableRootSlot::SymbolPropertyEntry { owner, sym_key });
    }
}

pub(super) fn note_symbol_property_attrs(owner: usize, sym_key: usize) {
    if crate::gc::young_log::addr_is_minor_collectible(owner)
        || crate::gc::young_log::addr_is_minor_relevant(sym_key)
    {
        note_symbol_slot(SymbolSideTableRootSlot::SymbolPropertyAttrs { owner, sym_key });
    }
}

pub(super) fn note_symbol_accessor(owner: usize, sym_key: usize, get_bits: u64, set_bits: u64) {
    if crate::gc::young_log::addr_is_minor_collectible(owner)
        || crate::gc::young_log::addr_is_minor_relevant(sym_key)
        || crate::gc::young_log::bits_are_minor_relevant(get_bits)
        || crate::gc::young_log::bits_are_minor_relevant(set_bits)
    {
        note_symbol_slot(SymbolSideTableRootSlot::SymbolAccessorProperty { owner, sym_key });
    }
}

pub(super) fn note_class_static_symbol(class_id: u32, sym_key: usize, value_bits: u64) {
    if crate::gc::young_log::addr_is_minor_relevant(sym_key)
        || crate::gc::young_log::bits_are_minor_relevant(value_bits)
    {
        note_symbol_slot(SymbolSideTableRootSlot::ClassStaticSymbol { class_id, sym_key });
    }
}

pub(super) fn note_symbol_pointer(ptr: usize) {
    if crate::gc::young_log::addr_is_minor_collectible(ptr) {
        note_symbol_slot(SymbolSideTableRootSlot::SymbolPointer { ptr });
    }
}

pub(crate) struct SymbolSideTableRootScanState {
    slots: Option<Vec<SymbolSideTableRootSlot>>,
    kept: Vec<SymbolSideTableRootSlot>,
    cursor: usize,
    young: bool,
    table_len: usize,
}

pub(crate) fn new_symbol_side_table_root_scan_state() -> Box<dyn std::any::Any> {
    Box::new(SymbolSideTableRootScanState {
        slots: None,
        kept: Vec::new(),
        cursor: 0,
        young: false,
        table_len: 0,
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
    if state.slots.is_none() {
        state.young = visitor.young_scope();
        if state.young {
            state.table_len = symbol_side_table_root_len();
            #[cfg(any(debug_assertions, test))]
            SYMBOL_SIDE_TABLE_YOUNG.with(|log| {
                log.borrow()
                    .debug_assert_logged(SYMBOL_YOUNG_LOG_NAME, &relevant_symbol_slots())
            });
            state.slots = Some(SYMBOL_SIDE_TABLE_YOUNG.with(|log| log.borrow_mut().take_sorted()));
        } else {
            let authoritative = symbol_side_table_root_snapshot();
            state.table_len = authoritative.len();
            let _ = SYMBOL_SIDE_TABLE_YOUNG.with(|log| log.borrow_mut().take_sorted());
            state.slots = Some(authoritative);
        }
    }
    let slots = state.slots.as_ref().expect("symbol slots initialized");
    while *remaining > 0 && state.cursor < slots.len() {
        if let Some(slot) = scan_symbol_side_table_root_slot(visitor, slots[state.cursor]) {
            state.kept.push(slot);
        }
        state.cursor += 1;
        *remaining -= 1;
    }
    let done = state.cursor >= slots.len();
    if done {
        let logged = slots.len() as u64;
        let kept = std::mem::take(&mut state.kept);
        let kept_len = kept.len() as u64;
        SYMBOL_SIDE_TABLE_YOUNG.with(|log| log.borrow_mut().extend(kept));
        crate::gc::young_log::note_walk(
            SYMBOL_YOUNG_LOG_NAME,
            crate::gc::young_log::YoungLogWalk {
                partial: state.young,
                logged,
                visited: state.cursor as u64,
                kept: kept_len,
                table_len: state.table_len as u64,
            },
        );
    }
    done
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

fn symbol_side_table_root_len() -> usize {
    // Exact only for diagnostics: counting the property vectors is itself a
    // whole-table walk, which the release minor must not pay merely to report
    // how much work it skipped.
    if !crate::gc::gc_diag_enabled() {
        return 0;
    }
    let properties = {
        let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
        guard.as_ref().map_or(0, |map| {
            map.len() + map.values().map(Vec::len).sum::<usize>()
        })
    };
    let attrs = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTY_ATTRS)
        .as_ref()
        .map_or(0, |map| map.len());
    let class_statics = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS)
        .as_ref()
        .map_or(0, |map| map.len());
    let pointers = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS)
        .as_ref()
        .map_or(0, |set| set.len());
    properties + attrs + accessors::accessor_property_count() + class_statics + pointers
}

fn collect_relevant_symbol_slots() -> Vec<SymbolSideTableRootSlot> {
    let mut slots = Vec::new();
    {
        let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
        if let Some(map) = guard.as_ref() {
            for (&owner, entries) in map {
                if crate::gc::young_log::addr_is_minor_collectible(owner) {
                    slots.push(SymbolSideTableRootSlot::SymbolPropertyOwner { owner });
                }
                for &(sym_key, value_bits) in entries {
                    if crate::gc::young_log::addr_is_minor_relevant(sym_key)
                        || crate::gc::young_log::bits_are_minor_relevant(value_bits)
                    {
                        slots.push(SymbolSideTableRootSlot::SymbolPropertyEntry { owner, sym_key });
                    }
                }
            }
        }
    }
    {
        let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTY_ATTRS);
        if let Some(map) = guard.as_ref() {
            slots.extend(map.keys().filter_map(|&(owner, sym_key)| {
                (crate::gc::young_log::addr_is_minor_collectible(owner)
                    || crate::gc::young_log::addr_is_minor_relevant(sym_key))
                .then_some(SymbolSideTableRootSlot::SymbolPropertyAttrs { owner, sym_key })
            }));
        }
    }
    slots.extend(
        accessors::relevant_accessor_property_keys()
            .into_iter()
            .map(
                |(owner, sym_key)| SymbolSideTableRootSlot::SymbolAccessorProperty {
                    owner,
                    sym_key,
                },
            ),
    );
    {
        let guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
        if let Some(map) = guard.as_ref() {
            slots.extend(
                map.iter()
                    .filter_map(|(&(class_id, sym_key), &value_bits)| {
                        (crate::gc::young_log::addr_is_minor_relevant(sym_key)
                            || crate::gc::young_log::bits_are_minor_relevant(value_bits))
                        .then_some(SymbolSideTableRootSlot::ClassStaticSymbol { class_id, sym_key })
                    }),
            );
        }
    }
    {
        let guard = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS);
        if let Some(set) = guard.as_ref() {
            slots.extend(set.iter().filter_map(|&ptr| {
                crate::gc::young_log::addr_is_minor_collectible(ptr)
                    .then_some(SymbolSideTableRootSlot::SymbolPointer { ptr })
            }));
        }
    }
    slots
}

#[cfg(any(debug_assertions, test))]
fn relevant_symbol_slots() -> Vec<SymbolSideTableRootSlot> {
    collect_relevant_symbol_slots()
}

fn rebuild_symbol_young_log() {
    let table_len = symbol_side_table_root_len();
    let relevant = collect_relevant_symbol_slots();
    let _ = SYMBOL_SIDE_TABLE_YOUNG.with(|log| log.borrow_mut().take_sorted());
    let kept = relevant.len() as u64;
    SYMBOL_SIDE_TABLE_YOUNG.with(|log| log.borrow_mut().extend(relevant));
    crate::gc::young_log::note_walk(
        SYMBOL_YOUNG_LOG_NAME,
        crate::gc::young_log::YoungLogWalk {
            partial: false,
            logged: table_len as u64,
            visited: table_len as u64,
            kept,
            table_len: table_len as u64,
        },
    );
}

fn scan_young_symbol_side_table_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let table_len = symbol_side_table_root_len();
    #[cfg(any(debug_assertions, test))]
    SYMBOL_SIDE_TABLE_YOUNG.with(|log| {
        log.borrow()
            .debug_assert_logged(SYMBOL_YOUNG_LOG_NAME, &relevant_symbol_slots())
    });
    let mut kept = SYMBOL_SIDE_TABLE_YOUNG.with(|log| log.borrow_mut().take_spare());
    let mut logged = 0_u64;
    loop {
        let batch = SYMBOL_SIDE_TABLE_YOUNG.with(|log| log.borrow_mut().take_sorted());
        if batch.is_empty() {
            break;
        }
        logged += batch.len() as u64;
        for slot in batch {
            if let Some(slot) = scan_symbol_side_table_root_slot(visitor, slot) {
                kept.push(slot);
            }
        }
    }
    let kept_len = kept.len() as u64;
    SYMBOL_SIDE_TABLE_YOUNG.with(|log| log.borrow_mut().extend(kept));
    crate::gc::young_log::note_walk(
        SYMBOL_YOUNG_LOG_NAME,
        crate::gc::young_log::YoungLogWalk {
            partial: true,
            logged,
            visited: logged,
            kept: kept_len,
            table_len: table_len as u64,
        },
    );
}

fn scan_symbol_side_table_root_slot(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    slot: SymbolSideTableRootSlot,
) -> Option<SymbolSideTableRootSlot> {
    match slot {
        SymbolSideTableRootSlot::SymbolPropertyOwner { owner } => {
            rewrite_symbol_property_owner_if_forwarded(visitor, owner).and_then(|owner| {
                crate::gc::young_log::addr_is_minor_collectible(owner)
                    .then_some(SymbolSideTableRootSlot::SymbolPropertyOwner { owner })
            })
        }
        SymbolSideTableRootSlot::SymbolPropertyEntry { owner, sym_key } => {
            // The preceding budget slice may already have rekeyed this
            // owner's map entry. Heal the snapshot's owner before looking up
            // the entry, otherwise every later slice searches the stale key
            // and skips both the symbol key and its value.
            let mut healed_owner = owner;
            visitor.visit_metadata_usize_slot(&mut healed_owner);
            let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
            let Some(map) = guard.as_mut() else {
                return None;
            };
            let lookup_owner = if map.contains_key(&healed_owner) {
                healed_owner
            } else {
                owner
            };
            let Some((entry_sym, value_bits)) = map
                .get_mut(&lookup_owner)
                .and_then(|entries| entries.iter_mut().find(|entry| entry.0 == sym_key))
            else {
                return None;
            };
            visitor.visit_usize_slot(entry_sym);
            visitor.visit_nanbox_u64_slot(value_bits);
            (crate::gc::young_log::addr_is_minor_relevant(*entry_sym)
                || crate::gc::young_log::bits_are_minor_relevant(*value_bits))
            .then_some(SymbolSideTableRootSlot::SymbolPropertyEntry {
                owner: lookup_owner,
                sym_key: *entry_sym,
            })
        }
        SymbolSideTableRootSlot::SymbolAccessorProperty { owner, sym_key } => {
            accessors::scan_symbol_accessor_root_slot(visitor, owner, sym_key).map(
                |(owner, sym_key)| SymbolSideTableRootSlot::SymbolAccessorProperty {
                    owner,
                    sym_key,
                },
            )
        }
        SymbolSideTableRootSlot::SymbolPropertyAttrs { owner, sym_key } => {
            rewrite_symbol_property_attrs_if_forwarded(visitor, owner, sym_key).and_then(
                |(owner, sym_key)| {
                    (crate::gc::young_log::addr_is_minor_collectible(owner)
                        || crate::gc::young_log::addr_is_minor_relevant(sym_key))
                    .then_some(SymbolSideTableRootSlot::SymbolPropertyAttrs { owner, sym_key })
                },
            )
        }
        SymbolSideTableRootSlot::ClassStaticSymbol { class_id, sym_key } => {
            rewrite_class_static_symbol_entry_if_forwarded(visitor, class_id, sym_key)
                .map(|sym_key| SymbolSideTableRootSlot::ClassStaticSymbol { class_id, sym_key })
        }
        SymbolSideTableRootSlot::SymbolPointer { ptr } => {
            rewrite_symbol_pointer_metadata_if_forwarded(visitor, ptr).and_then(|ptr| {
                crate::gc::young_log::addr_is_minor_collectible(ptr)
                    .then_some(SymbolSideTableRootSlot::SymbolPointer { ptr })
            })
        }
    }
}

fn rewrite_symbol_property_owner_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    owner: usize,
) -> Option<usize> {
    let mut new_owner = owner;
    if visitor.visit_metadata_usize_slot(&mut new_owner) && new_owner != owner {
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
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    guard
        .as_ref()
        .is_some_and(|map| map.contains_key(&new_owner))
        .then_some(new_owner)
}

fn rewrite_symbol_property_attrs_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    owner: usize,
    sym_key: usize,
) -> Option<(usize, usize)> {
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTY_ATTRS);
    let Some(map) = guard.as_mut() else {
        return None;
    };
    if !map.contains_key(&(owner, sym_key)) {
        return None;
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
    map.contains_key(&(new_owner, new_sym_key))
        .then_some((new_owner, new_sym_key))
}

fn rewrite_class_static_symbol_entry_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    class_id: u32,
    sym_key: usize,
) -> Option<usize> {
    let mut guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
    let Some(map) = guard.as_mut() else {
        return None;
    };
    let Some(value_bits) = map.get_mut(&(class_id, sym_key)) else {
        return None;
    };
    let mut new_sym_key = sym_key;
    let moved = visitor.visit_usize_slot(&mut new_sym_key);
    visitor.visit_nanbox_u64_slot(value_bits);
    if moved && new_sym_key != sym_key {
        if let Some(value_bits) = map.remove(&(class_id, sym_key)) {
            map.insert((class_id, new_sym_key), value_bits);
        }
    }
    map.get(&(class_id, new_sym_key)).and_then(|value_bits| {
        (crate::gc::young_log::addr_is_minor_relevant(new_sym_key)
            || crate::gc::young_log::bits_are_minor_relevant(*value_bits))
        .then_some(new_sym_key)
    })
}

fn rewrite_symbol_pointer_metadata_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    ptr: usize,
) -> Option<usize> {
    let mut new_ptr = ptr;
    if visitor.visit_metadata_usize_slot(&mut new_ptr) && new_ptr != ptr {
        let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS);
        if let Some(set) = guard.as_mut() {
            set.remove(&ptr);
            if new_ptr != 0 {
                insert_symbol_pointer_in_set(set, new_ptr);
            }
        }
    }
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS);
    guard
        .as_ref()
        .is_some_and(|set| set.contains(&new_ptr))
        .then_some(new_ptr)
}

#[cfg(test)]
pub(crate) fn test_clear_symbol_side_table_roots() {
    SYMBOL_SIDE_TABLE_YOUNG.with(|log| log.borrow_mut().clear());
    *crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES) = None;
    *crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTY_ATTRS) = None;
    *crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS) = None;
    *CLASS_STATIC_SYMBOL_ORDER.lock().unwrap() = None;
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
        let mut set = new_ptr_hash_set();
        for ptr in persistent {
            note_symbol_pointer(ptr);
            insert_symbol_pointer_in_set(&mut set, ptr);
        }
        *guard = Some(set);
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
        // Root-scanner tests deliberately use synthetic addresses, including
        // an unaligned sentinel. Seed only the root table they exercise;
        // production registration additionally reads SymbolHeader::id for
        // [[OwnPropertyKeys]] ordering and therefore requires a real Symbol.
        note_class_static_symbol(class_id, sym_key, value_bits);
        CLASS_STATIC_SYMBOLS_LATCH.arm();
        let mut guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        guard
            .as_mut()
            .unwrap()
            .insert((class_id, sym_key), value_bits);
        drop(guard);
        publish_symbol_side_table_root_edges(sym_key, value_bits);
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

#[cfg(test)]
mod young_log_sabotage_tests {
    use super::*;

    #[test]
    fn symbol_log_rederivation_rejects_a_suppressed_property_writer() {
        let _lock = crate::gc::global_side_table_test_lock();
        test_clear_symbol_side_table_roots();
        let owner = crate::object::js_object_alloc(0, 0) as usize;
        let sym_bits = unsafe { crate::symbol::js_symbol_new_empty() }.to_bits();
        let sym_key = (sym_bits & POINTER_MASK) as usize;
        TEST_SUPPRESS_SYMBOL_YOUNG_NOTE.with(|flag| flag.set(true));
        store_object_symbol_property_root(owner, sym_key, 7.0_f64.to_bits());
        TEST_SUPPRESS_SYMBOL_YOUNG_NOTE.with(|flag| flag.set(false));
        let missed = std::panic::catch_unwind(|| {
            SYMBOL_SIDE_TABLE_YOUNG.with(|log| {
                log.borrow()
                    .debug_assert_logged(SYMBOL_YOUNG_LOG_NAME, &relevant_symbol_slots())
            });
        });
        test_clear_symbol_side_table_roots();
        assert!(
            missed.is_err(),
            "sabotage: suppressing the property-store note must trip completeness"
        );
    }
}
