//! A static literal seed keeps naming the one canonical list of its keys
//! across collections.
//!
//! A seed publishes its whole key list early, with its prefixes as
//! unpublished witnesses on the seed's own array. A receiver that later grows
//! through those prefixes publishes them on ITS backing. When that receiver
//! dies, its backing dies with them. The prune used to free those prefix nodes
//! and orphan the seeded list under them, so the next walk of the same keys
//! (the literal's own module init) published a second array. The keys array is
//! a shape identity fact: the module-init mint of the static id then missed the
//! seeded record, minted a counter id (a silent static miss) or, for a literal
//! born wide, ABORTED ("the static ShapeId ... was refused by the shape mint").
//! OpenCode's TUI died on that at startup.
//!
//! Invariant: after any collection, walking a live list's keys from the root
//! reaches that list, and the static mint of the seeded facts returns the
//! seeded id. Both orders of seed and growth are covered.

use super::super::*;
use super::support::*;
use crate::gc::RuntimeHandleScope;
use crate::object::shapes;
use crate::object::static_shapes::{canonical_keys_for_names, js_shape_seed_plain};

fn register_object_model_scanners() {
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    gc_register_mutable_root_scanner(crate::object::canonical_keys::scan_canonical_keys_roots_mut);
}

/// Precise roots only, so the dead receiver really dies.
struct NoConservativeScan(Option<crate::gc::roots::ConservativeStackScanMode>);

impl NoConservativeScan {
    fn new() -> Self {
        Self(crate::gc::roots::set_conservative_stack_scan_override(
            Some(crate::gc::roots::ConservativeStackScanMode::Disabled),
        ))
    }
}

impl Drop for NoConservativeScan {
    fn drop(&mut self) {
        crate::gc::roots::set_conservative_stack_scan_override(self.0);
    }
}

/// A receiver that grows `names` one key at a time and dies at once.
fn grow_and_drop(names: &[&str]) {
    let scope = RuntimeHandleScope::new();
    let o = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    for (i, n) in names.iter().enumerate() {
        let k = crate::string::js_string_from_bytes(n.as_ptr(), n.len() as u32);
        o.with_mut_ptr(|p| crate::object::js_object_set_field_by_name(p, k, i as f64));
    }
}

fn seed(requested: u32, names: &[&str]) -> u32 {
    let packed: Vec<u8> = names
        .iter()
        .flat_map(|n| n.bytes().chain(std::iter::once(0)))
        .collect();
    let n = names.len() as u32;
    js_shape_seed_plain(requested, packed.as_ptr(), packed.len() as u32, n, n, 0)
}

/// The literal's module init: its class keys array, then the static mint.
fn module_init_mint(requested: u32, names: &[&str]) -> u32 {
    let packed: Vec<u8> = names
        .iter()
        .flat_map(|n| n.bytes().chain(std::iter::once(0)))
        .collect();
    let n = names.len() as u32;
    let arr = crate::object::js_build_class_keys_array(
        0x5_EED0 + requested % 64,
        n,
        packed.as_ptr(),
        packed.len() as u32,
        0,
    );
    crate::object::static_shapes::js_object_shape_id_for_class_keys_static(
        arr as u64, n, n, 0, requested, 0,
    )
}

fn collect_all() {
    let _ = collect_minor_trace(GcTriggerKind::Direct);
    crate::gc::js_gc_collect();
    crate::gc::js_gc_collect();
}

fn check_seed_survives(order_seed_first: bool, tag: &str, requested: u32) {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_object_model_scanners();
    crate::object::canonical_keys::reset_for_test();
    let p = format!("{tag}_p");
    let q = format!("{tag}_q");
    let r = format!("{tag}_r");
    let x = format!("{tag}_x");
    let literal = [p.as_str(), q.as_str(), r.as_str()];
    // The transient receiver shares the literal's first two keys, then forks.
    let transient = [p.as_str(), q.as_str(), x.as_str()];
    let id = if order_seed_first {
        let id = seed(requested, &literal);
        grow_and_drop(&transient);
        id
    } else {
        grow_and_drop(&transient);
        seed(requested, &literal)
    };
    assert_eq!(id, requested, "premise: the seed adopts its static id");
    let published_before = crate::object::canonical_keys::published_lists_for_test().len();
    let _precise = NoConservativeScan::new();
    collect_all();
    let published_after = crate::object::canonical_keys::published_lists_for_test().len();
    assert!(
        published_after < published_before,
        "premise: the transient receiver's lists died ({published_before} -> {published_after})"
    );
    let seeded = shapes::shape_descriptor_by_id(id).map(|d| d.keys);
    let names: Vec<&[u8]> = literal.iter().map(|n| n.as_bytes()).collect();
    let walked = unsafe { canonical_keys_for_names(&names) }.arr() as u64;
    assert_eq!(
        seeded,
        Some(walked),
        "INVARIANT: a walk of the seeded keys reaches the seeded list, not a duplicate"
    );
    assert_eq!(
        module_init_mint(requested, &literal),
        requested,
        "INVARIANT: the module-init mint of the seeded facts returns the static id"
    );
}

#[test]
fn a_seed_whose_prefixes_a_dead_receiver_published_stays_reachable() {
    check_seed_survives(true, "sk1", shapes::SHAPE_ID_BASE + 0x7_1241);
}

#[test]
fn a_seed_that_forked_from_a_dead_receivers_prefix_stays_reachable() {
    check_seed_survives(false, "sk2", shapes::SHAPE_ID_BASE + 0x7_1242);
}
