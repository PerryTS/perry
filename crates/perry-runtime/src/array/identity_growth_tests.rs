//! #13792: identity-keyed tables find an array under either of the two
//! addresses growth gives it (the forwarding stub a stored key holds, the new
//! head a refreshed binding holds).

use super::*;

fn boxed(arr: *const ArrayHeader) -> f64 {
    f64::from_bits(crate::value::POINTER_TAG | (arr as u64 & crate::value::POINTER_MASK))
}

const TRUE_BITS: u64 = 0x7FFC_0000_0000_0004;

/// A fresh array and its grown head; the old head is a forwarding stub.
fn grown_pair() -> (*mut ArrayHeader, *mut ArrayHeader) {
    let initial = js_array_alloc_literal(1);
    let head = js_array_grow(initial, 4096);
    assert_ne!(head, initial, "growth past capacity must move the array");
    assert!(arrays_share_identity(
        boxed(initial).to_bits(),
        boxed(head).to_bits()
    ));
    (initial, head)
}

#[test]
fn distinct_arrays_do_not_share_identity() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let a = js_array_alloc_literal(1);
    let b = js_array_alloc_literal(1);
    assert!(!arrays_share_identity(
        boxed(a).to_bits(),
        boxed(b).to_bits()
    ));
    assert!(!arrays_share_identity(boxed(a).to_bits(), 1.0f64.to_bits()));
}

#[test]
fn object_is_sees_one_array_through_growth() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let (initial, head) = grown_pair();
    let is = crate::object::js_object_is(boxed(initial), boxed(head));
    assert_eq!(is.to_bits(), TRUE_BITS);
}

#[test]
fn weakmap_and_weakset_find_a_grown_array_key() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let initial = js_array_alloc_literal(1);
    let wm = f64::from_bits(crate::value::POINTER_TAG | crate::weakref::js_weakmap_new() as u64);
    let ws = f64::from_bits(crate::value::POINTER_TAG | crate::weakref::js_weakset_new() as u64);
    // Other array keys first, so the table grows and rehashes by token.
    for i in 0..32 {
        crate::weakref::js_weakmap_set(wm, boxed(js_array_alloc_literal(1)), i as f64);
    }
    crate::weakref::js_weakmap_set(wm, boxed(initial), 7.0);
    crate::weakref::js_weakset_add(ws, boxed(initial));
    let head = js_array_grow(initial, 4096);
    assert_ne!(head, initial);
    assert_eq!(crate::weakref::js_weakmap_get(wm, boxed(head)), 7.0);
    assert_eq!(crate::weakref::js_weakmap_get(wm, boxed(initial)), 7.0);
    assert_eq!(
        crate::weakref::js_weakset_has(ws, boxed(head)).to_bits(),
        TRUE_BITS
    );
    // Setting through the new head replaces, never duplicates.
    crate::weakref::js_weakmap_set(wm, boxed(head), 8.0);
    assert_eq!(crate::weakref::js_weakmap_get(wm, boxed(initial)), 8.0);
    crate::weakref::js_weakmap_delete(wm, boxed(head));
    assert_ne!(
        crate::weakref::js_weakmap_has(wm, boxed(initial)).to_bits(),
        TRUE_BITS
    );
    // An array never inserted has no token: a definite miss.
    assert_ne!(
        crate::weakref::js_weakmap_has(wm, boxed(js_array_alloc_literal(1))).to_bits(),
        TRUE_BITS
    );
}

#[test]
fn small_and_large_maps_find_a_grown_array_key() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    for fillers in [0usize, 64] {
        let map = crate::map::js_map_alloc(4);
        for i in 0..fillers {
            crate::map::js_map_set(map, boxed(js_array_alloc_literal(1)), i as f64);
        }
        let initial = js_array_alloc_literal(1);
        crate::map::js_map_set(map, boxed(initial), 9.0);
        let head = js_array_grow(initial, 4096);
        assert_ne!(head, initial);
        assert_eq!(
            crate::map::js_map_get(map, boxed(head)),
            9.0,
            "fillers={fillers}"
        );
        let size = crate::map::js_map_size(map);
        crate::map::js_map_set(map, boxed(head), 10.0);
        assert_eq!(
            crate::map::js_map_size(map),
            size,
            "fillers={fillers}: no duplicate"
        );
        assert_eq!(crate::map::js_map_get(map, boxed(initial)), 10.0);
        assert_eq!(crate::map::js_map_delete(map, boxed(head)), 1);
        assert_eq!(
            crate::map::js_map_has(map, boxed(initial)),
            0,
            "fillers={fillers}"
        );
    }
}

extern "C" fn finreg_growth_test_callback(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    _held: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[test]
fn finalization_registry_unregisters_a_grown_array_token() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let cb = crate::closure::js_closure_alloc(crate::fn_info!(finreg_growth_test_callback, 1), 0);
    let reg = f64::from_bits(
        crate::value::POINTER_TAG
            | crate::weakref::js_finreg_new(f64::from_bits(
                crate::value::POINTER_TAG | (cb as u64 & crate::value::POINTER_MASK),
            )) as u64,
    );
    let target = js_array_alloc_literal(1);
    let initial = js_array_alloc_literal(1);
    crate::weakref::js_finreg_register(reg, boxed(target), 1.0, boxed(initial));
    let head = js_array_grow(initial, 4096);
    assert_ne!(head, initial);
    assert_eq!(
        crate::weakref::js_finreg_unregister(reg, boxed(head)).to_bits(),
        TRUE_BITS,
        "the token registered before growth is the grown array"
    );
    assert_ne!(
        crate::weakref::js_finreg_unregister(reg, boxed(initial)).to_bits(),
        TRUE_BITS
    );
}

#[test]
fn small_map_finds_a_head_key_by_its_stub_and_a_stub_key_by_its_head() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    // Stored head, forwarded probe: the probe resolves to the stored bits.
    let (stub, head) = grown_pair();
    let map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(map, boxed(js_array_alloc_literal(1)), 1.0);
    crate::map::js_map_set(map, boxed(head), 2.0);
    assert_eq!(crate::map::js_map_get(map, boxed(stub)), 2.0);
    assert_eq!(crate::map::js_map_size(map), 2);
    // A stub inserted into a small map that has already proved (at the
    // current growth epoch) that it holds no stub key: the insert must
    // withdraw that proof, or the head misses.
    let (stub2, head2) = grown_pair();
    let other = crate::map::js_map_alloc(4);
    crate::map::js_map_set(other, boxed(js_array_alloc_literal(1)), 1.0);
    crate::map::js_map_set(other, boxed(stub2), 3.0);
    assert_eq!(crate::map::js_map_get(other, boxed(head2)), 3.0);
    crate::map::js_map_set(other, boxed(head2), 4.0);
    assert_eq!(crate::map::js_map_size(other), 2, "no duplicate");
}

#[test]
fn a_map_grown_large_finds_an_array_key_stored_while_small() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let map = crate::map::js_map_alloc(4);
    let initial = js_array_alloc_literal(1);
    crate::map::js_map_set(map, boxed(initial), 7.0);
    for i in 0..64 {
        crate::map::js_map_set(map, boxed(js_array_alloc_literal(1)), i as f64);
    }
    let head = js_array_grow(initial, 4096);
    assert_ne!(head, initial);
    assert_eq!(crate::map::js_map_get(map, boxed(head)), 7.0);
    assert_eq!(crate::map::js_map_has(map, boxed(initial)), 1);
}

#[test]
fn a_small_map_keeps_a_stored_stub_through_store_growth() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    // A store is born proved (no keys). The proof must not be reborn when a
    // small map that holds a stub key moves to a larger store (capacity 4 -> 8).
    let (stub, head) = grown_pair();
    let map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(map, boxed(stub), 5.0);
    for i in 0..4 {
        crate::map::js_map_set(map, boxed(js_array_alloc_literal(1)), i as f64);
    }
    assert_eq!(crate::map::js_map_size(map), 5);
    assert_eq!(crate::map::js_map_get(map, boxed(head)), 5.0);
}
