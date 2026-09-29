//! The copying drain's plain-object scan (`gc/copying_object_scan.rs`) is a
//! second enumeration of an ordinary object's slots, so it is pinned three ways:
//! it is actually TAKEN by a minor over ordinary objects (a fast path nothing
//! reaches would make every other test here vacuous), every child it visits is
//! evacuated and rewritten, and a plan that drops a slot is REFUSED by the
//! generic-walk cross-check that runs in test and debug-assertion builds.

use super::super::*;
use super::support::*;
use crate::gc::copying_object_scan::sabotage;

fn string_bytes(addr: usize) -> Vec<u8> {
    unsafe {
        let s = addr as *const crate::StringHeader;
        std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize).to_vec()
    }
}

const FIELDS: usize = 3;

/// A rooted young object whose every field holds a young string, then a minor.
/// `Ok((plan attempts, every child moved and intact))`; `Err` is the
/// collection thread's panic message.
fn minor_over_a_plain_object(sabotaged: bool) -> Result<(u64, bool), String> {
    std::thread::spawn(move || {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        let _roots = ShadowAndGlobalRootResetGuard;
        let (parent, fields) = unsafe { alloc_nursery_test_object(FIELDS as u32) };
        let mut children = Vec::new();
        for i in 0..FIELDS {
            let child = young_leaf();
            unsafe { *fields.add(i) = string_bits(child) };
            children.push((child, string_bytes(child)));
        }
        js_shadow_slot_set(0, ptr_bits(parent as usize));
        let before = sabotage::plan_attempts();
        {
            let _sabotage = sabotaged.then(sabotage::DropTopPayloadSlot::arm);
            let _ = gc_collect_minor();
        }
        let attempts = sabotage::plan_attempts() - before;
        let parent_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        let fields_after = unsafe {
            (parent_after as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
                as *const u64
        };
        let intact = children.iter().enumerate().all(|(i, (old, bytes))| {
            let word = unsafe { *fields_after.add(i) };
            let now = (word & POINTER_MASK) as usize;
            // Checked before any read through `now`: a stale word names from-space.
            now != *old && string_bytes(now) == *bytes
        });
        (attempts, intact)
    })
    .join()
    .map_err(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    })
}

#[test]
fn a_minor_scans_a_plain_object_through_its_plan_and_moves_every_child() {
    let (attempts, intact) = minor_over_a_plain_object(false).expect("minor must not panic");
    assert!(
        attempts > 0,
        "premise: the minor must have taken the plain-object path at least once"
    );
    assert!(intact, "every field's young child must be evacuated and its word rewritten");
}

#[test]
fn a_plan_that_drops_a_payload_slot_is_refused_by_the_generic_walk_cross_check() {
    let outcome = minor_over_a_plain_object(true);
    assert!(
        matches!(&outcome, Err(message) if message.contains("copying_object_scan")),
        "a plan missing the top payload slot must be caught by the cross-check; got {outcome:?}"
    );
}
