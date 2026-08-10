//! Receiver-tag gating of the `Map`/`Set` registry probes (#7765).
//!
//! `js_array_get_f64` and `js_array_length` used to ask both collection
//! registries "is this receiver a Set? a Map?" on every element read of an
//! ordinary array. Once a program creates one `Map` the #7474 monotone latch is
//! armed and both probes are real work — a thread-local resolution plus a hash,
//! per read, to prove an array is not a Map. They are now gated on the
//! receiver's own `GcHeader.obj_type`.
//!
//! These tests assert THE SUBJECT, not just the answer. The registry is a
//! correct fallback, so a test that only compared values would still pass with
//! the gate deleted — case 4 of CLAUDE.md's "four ways a gate can be unable to
//! fail". `TEST_{MAP,SET}_REGISTRY_PROBES` count every entry into
//! `is_registered_map` / `is_registered_set`, and the plain-array case asserts
//! those counters do not move. Delete either gate and that assertion fails.
//!
//! The ABA case has its own test. The tag is ABA-proof because it lives INSIDE
//! the candidate bytes: whatever allocation owns those bytes next stamps its own
//! `obj_type` through `arena_alloc_gc` before the pointer is handed out, so a
//! recycled address answers for its new owner. That is the property an
//! address-keyed negative memo could not have (#7755), and
//! `a_stale_registry_entry_over_recycled_bytes_does_not_read_as_a_map` plants
//! exactly that state.

use super::*;
use crate::map::{js_map_alloc, js_map_set, js_map_size, MapHeader};
use crate::set::{js_set_add, js_set_alloc, js_set_size, SetHeader};

fn probes() -> (u64, u64) {
    (
        crate::map::test_map_registry_probe_count(),
        crate::set::test_set_registry_probe_count(),
    )
}

fn dense(values: &[f64]) -> *mut ArrayHeader {
    let arr = js_array_alloc(values.len().max(1) as u32);
    let mut cur = arr;
    for v in values {
        cur = js_array_push_f64(cur, *v);
    }
    cur
}

/// Arm both #7474 latches, so nothing in this file is measuring the
/// "no collection has ever existed" fast-out instead of the tag gate.
fn arm_both_registries() -> (*mut MapHeader, *mut SetHeader) {
    let map = js_map_alloc(4);
    js_map_set(map, 1.0, 10.0);
    let set = js_set_alloc(4);
    js_set_add(set, 5.0);
    assert!(
        crate::map::is_registered_map(map as usize),
        "the map registry must be armed for these tests to mean anything"
    );
    assert!(
        crate::set::is_registered_set(set as usize),
        "the set registry must be armed for these tests to mean anything"
    );
    (map, set)
}

unsafe fn gc_obj_type(addr: usize) -> u8 {
    let header = (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    (*header).obj_type
}

unsafe fn set_gc_obj_type(addr: usize, obj_type: u8) {
    let header = (addr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
    (*header).obj_type = obj_type;
}

#[test]
fn plain_array_element_reads_never_probe_the_collection_registries() {
    let (map, set) = arm_both_registries();
    assert_eq!(js_map_size(map), 1);
    assert_eq!(js_set_size(set), 1);

    let arr = dense(&[1.0, 2.0, 3.0, 4.0]);
    // Prime anything lazily built on first touch, then measure.
    let _ = js_array_length(arr);
    let _ = js_array_get_f64(arr, 0);

    let before = probes();
    let mut sum = 0.0;
    for _ in 0..64 {
        let len = js_array_length(arr);
        assert_eq!(len, 4);
        for i in 0..len {
            sum += js_array_get_f64(arr, i);
        }
    }
    let after = probes();

    assert_eq!(sum, 64.0 * 10.0, "the reads must still return the elements");
    assert_eq!(
        after, before,
        "a GC_TYPE_ARRAY receiver must never reach is_registered_map / \
         is_registered_set — remove either receiver-tag gate in \
         js_array_get_f64 / js_array_length and this is what fails"
    );
}

#[test]
fn a_live_set_receiver_still_reads_its_elements_through_the_registry() {
    let (_map, set) = arm_both_registries();
    js_set_add(set, 6.0);
    js_set_add(set, 7.0);
    assert_eq!(js_set_size(set), 3);
    assert_eq!(
        unsafe { gc_obj_type(set as usize) },
        crate::gc::GC_TYPE_SET,
        "js_set_alloc must stamp GC_TYPE_SET — the gate reads exactly this byte"
    );

    let as_array = set as *const ArrayHeader;
    let before = probes();
    assert_eq!(js_array_length(as_array), 3);
    assert_eq!(js_array_get_f64(as_array, 0), 5.0);
    assert_eq!(js_array_get_f64(as_array, 1), 6.0);
    assert_eq!(js_array_get_f64(as_array, 2), 7.0);
    let after = probes();

    assert!(
        after.1 > before.1,
        "a GC_TYPE_SET receiver must still be confirmed against the \
         authoritative registry, not served on the tag alone"
    );
}

#[test]
fn a_live_map_receiver_still_reports_its_size_through_the_registry() {
    let (map, _set) = arm_both_registries();
    js_map_set(map, 2.0, 20.0);
    assert_eq!(js_map_size(map), 2);
    assert_eq!(
        unsafe { gc_obj_type(map as usize) },
        crate::gc::GC_TYPE_MAP,
        "js_map_alloc must stamp GC_TYPE_MAP — the gate reads exactly this byte"
    );

    let as_array = map as *const ArrayHeader;
    let before = probes();
    assert_eq!(js_array_length(as_array), 2);
    assert_eq!(js_array_get_f64(as_array, 0), 1.0, "entry 0's key");
    assert_eq!(js_array_get_f64(as_array, 1), 2.0, "entry 1's key");
    let after = probes();

    assert!(
        after.0 > before.0,
        "a GC_TYPE_MAP receiver must still be confirmed against the \
         authoritative registry, not served on the tag alone"
    );
}

/// The ABA case #7755 named: an address that WAS a `Map` and is now something
/// else, while the registry has not caught up.
///
/// The bytes are re-stamped exactly as `arena_alloc_gc` would when it hands the
/// address to a plain array — `obj_type` first, then the new object's own words
/// — and the registry deliberately still holds the old entry, which is the
/// worst case a sweep-ordering bug could produce. The answer must come from the
/// bytes.
///
/// Sabotage: delete the `try_read_gc_header` confirmation at the end of
/// `map::is_registered_map` and this fails — the stale registry entry alone
/// then reports `true`, and the element read serves `MapHeader::entries` as if
/// the recycled array were a Map.
#[test]
fn a_stale_registry_entry_over_recycled_bytes_does_not_read_as_a_map() {
    let (map, _set) = arm_both_registries();
    js_map_set(map, 2.0, 20.0);
    js_map_set(map, 3.0, 30.0);
    assert_eq!(js_map_size(map), 3);

    let addr = map as usize;
    assert!(
        crate::map::is_registered_map(addr),
        "precondition: the address is a registered Map"
    );

    // Recycle the bytes into a two-element dense array, registry untouched.
    unsafe {
        set_gc_obj_type(addr, crate::gc::GC_TYPE_ARRAY);
        let recycled = addr as *mut ArrayHeader;
        (*recycled).length = 2;
        (*recycled).capacity = 2;
        let elements = (addr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        std::ptr::write(elements, 111.0);
        std::ptr::write(elements.add(1), 222.0);
    }

    assert!(
        !crate::map::is_registered_map(addr),
        "the tag lives in the recycled bytes, so the address must stop \
         answering as a Map the instant another allocation owns them — a \
         stale registry entry must not override that"
    );
    let as_array = addr as *const ArrayHeader;
    assert_eq!(
        js_array_length(as_array),
        2,
        "js_array_length must report the recycled array's length, not the \
         dead Map's size"
    );
    assert_eq!(js_array_get_f64(as_array, 0), 111.0);
    assert_eq!(js_array_get_f64(as_array, 1), 222.0);

    // Restore the Map shape so teardown's side-allocation release stays sound.
    unsafe {
        set_gc_obj_type(addr, crate::gc::GC_TYPE_MAP);
        (*map).size = 3;
        (*map).capacity = 4;
    }
    assert!(crate::map::is_registered_map(addr));
    assert_eq!(js_map_size(map), 3);
}

/// `keys_array_slot` must be the general getter, minus the work the field-get
/// funnel already did — and must refuse every shape it cannot serve on those
/// terms rather than guess. Both directions are asserted against the fallback
/// counter, so "stopped applying" and "started swallowing" are equally red.
#[test]
fn keys_array_slot_matches_the_general_getter_and_delegates_what_it_cannot_serve() {
    let dense_keys = dense(&[10.0, 20.0, 30.0]);

    let before = crate::array::test_keys_array_slot_fallbacks();
    for i in 0..3u32 {
        let fast = unsafe { crate::array::keys_array_slot(dense_keys, i) };
        let general = crate::array::js_array_get(dense_keys, i);
        assert_eq!(
            fast.bits(),
            general.bits(),
            "slot {i} must read identically through both paths"
        );
    }
    assert_eq!(
        crate::array::test_keys_array_slot_fallbacks(),
        before,
        "a dense, descriptor-free keys array is exactly what the fast path \
         exists for — it must not delegate"
    );

    // Out of range, and a hole, both delegate: the general getter walks the
    // prototype chain for those and the dense words cannot answer.
    let before = crate::array::test_keys_array_slot_fallbacks();
    let oob = unsafe { crate::array::keys_array_slot(dense_keys, 7) };
    assert_eq!(oob.bits(), crate::array::js_array_get(dense_keys, 7).bits());
    assert_eq!(
        crate::array::test_keys_array_slot_fallbacks(),
        before + 1,
        "an out-of-range index must reach the general getter"
    );

    let holey = js_array_alloc_with_length(3);
    js_array_set_f64(holey, 1, 42.0);
    let before = crate::array::test_keys_array_slot_fallbacks();
    let hole = unsafe { crate::array::keys_array_slot(holey, 0) };
    assert_eq!(hole.bits(), crate::array::js_array_get(holey, 0).bits());
    assert_eq!(
        crate::array::test_keys_array_slot_fallbacks(),
        before + 1,
        "a HOLE reads through the prototype chain, so it must delegate"
    );
    let filled = unsafe { crate::array::keys_array_slot(holey, 1) };
    assert_eq!(filled.bits(), crate::array::js_array_get(holey, 1).bits());

    // A null / low pointer must delegate rather than dereference.
    let before = crate::array::test_keys_array_slot_fallbacks();
    let _ = unsafe { crate::array::keys_array_slot(std::ptr::null(), 0) };
    assert_eq!(
        crate::array::test_keys_array_slot_fallbacks(),
        before + 1,
        "a null keys pointer must delegate, never be dereferenced"
    );
}

/// The invariant the whole gate rests on: a registered collection's address IS
/// its `arena_alloc_gc` header, so its `obj_type` is a complete answer.
///
/// `js_map_alloc` / `js_set_alloc` are the single registration site for each,
/// and both grow their side buffer by `realloc` without moving the header — so
/// this must survive growth too. A future registration path that forgot the tag
/// would make the fast negative wrong, and this is what would catch it.
#[test]
fn every_registered_collection_address_carries_its_own_type_tag() {
    let mut maps = Vec::new();
    let mut sets = Vec::new();
    for n in 0..8u32 {
        let map = js_map_alloc(if n % 3 == 0 { 0 } else { n });
        for k in 0..(n * 4 + 1) {
            js_map_set(map, k as f64, (k * 2) as f64);
        }
        maps.push(map);

        let set = js_set_alloc(if n % 2 == 0 { 0 } else { n });
        for k in 0..(n * 4 + 1) {
            js_set_add(set, (k + 1000) as f64);
        }
        sets.push(set);
    }

    for map in maps {
        assert_eq!(
            unsafe { gc_obj_type(map as usize) },
            crate::gc::GC_TYPE_MAP,
            "registered Map at {map:p} must carry GC_TYPE_MAP"
        );
        assert!(crate::map::is_registered_map(map as usize));
    }
    for set in sets {
        assert_eq!(
            unsafe { gc_obj_type(set as usize) },
            crate::gc::GC_TYPE_SET,
            "registered Set at {set:p} must carry GC_TYPE_SET"
        );
        assert!(crate::set::is_registered_set(set as usize));
    }
}
