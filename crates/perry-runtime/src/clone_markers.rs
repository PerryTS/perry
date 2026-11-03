use crate::gc::{RuntimeHandleScope, RuntimeRootVisitor};
use crate::value::JSValue;
use std::cell::Cell;

const UNTRANSFERABLE: u8 = 1;
const UNCLONEABLE: u8 = 2;

crate::perry_thread_local! {
    static MARKERS: Cell<Option<u64>> = const { Cell::new(None) };
}

fn scan_clone_marker_roots_mut(visitor: &mut RuntimeRootVisitor<'_>) {
    MARKERS.with(|slot| {
        if let Some(mut bits) = slot.get() {
            visitor.visit_nanbox_u64_slot(&mut bits);
            slot.set(Some(bits));
        }
    });
}

fn is_marker_target(value: f64) -> bool {
    if !crate::weakref::is_valid_weak_target(value) {
        return false;
    }
    !JSValue::from_bits(value.to_bits()).is_pointer()
        || !crate::symbol::is_registered_symbol(crate::value::js_nanbox_get_pointer(value) as usize)
}

fn marker_flags(value: f64) -> u8 {
    let Some(bits) = MARKERS.with(Cell::get) else {
        return 0;
    };
    if !is_marker_target(value) {
        return 0;
    }
    let flags = crate::weakref::js_weakmap_get(f64::from_bits(bits), value);
    let flags = JSValue::from_bits(flags.to_bits());
    if flags.is_undefined() {
        0
    } else {
        flags.as_number() as u8
    }
}

fn set_marker_flag(value: f64, flag: u8) {
    if !is_marker_target(value) {
        return;
    }
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "runtime:clone_markers",
        scan_clone_marker_roots_mut,
    );
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    if MARKERS.with(Cell::get).is_none() {
        let map = crate::weakref::js_weakmap_new();
        MARKERS.with(|slot| slot.set(Some(crate::value::js_nanbox_pointer(map as i64).to_bits())));
    }
    let flags = marker_flags(value.get_nanbox_f64()) | flag;
    let map = f64::from_bits(MARKERS.with(Cell::get).unwrap());
    crate::weakref::js_weakmap_set(map, value.get_nanbox_f64(), flags as f64);
}

pub fn mark_untransferable(value: f64) {
    set_marker_flag(value, UNTRANSFERABLE);
}
pub fn mark_uncloneable(value: f64) {
    set_marker_flag(value, UNCLONEABLE);
}
pub fn is_untransferable(value: f64) -> bool {
    marker_flags(value) & UNTRANSFERABLE != 0
}
pub(crate) fn is_uncloneable(value: f64) -> bool {
    marker_flags(value) & UNCLONEABLE != 0
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    MARKERS.with(|slot| slot.set(None));
}

#[cfg(test)]
pub(crate) fn entry_count_for_tests() -> usize {
    let Some(bits) = MARKERS.with(Cell::get) else {
        return 0;
    };
    crate::weakref::weak_collection_entries(crate::value::js_nanbox_get_pointer(f64::from_bits(
        bits,
    )) as *const _)
    .len()
}
