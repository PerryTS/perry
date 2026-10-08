//! Correctness tests for the monotone side-table probe latches.
//!
//! The latches in [`crate::registry_latch`] make "is this value special?" free
//! for programs that never use the feature. Speed is the easy half; the hard
//! half is that a program which *does* use the feature must still work, and in
//! particular must still work when the feature is first used **after** the
//! probe's idle fast path has already been taken. Every test below therefore
//! takes the fast path first and only then registers.
//!
//! The `latch_semantics` tests at the bottom prove the ordering rule is
//! load-bearing rather than decorative: they model both orderings of
//! arm-vs-insert and show that only the wrong one can be observed as
//! "idle while the table already holds the entry".

use crate::registry_latch::RegistryLatch;

/// A heap-plausible address that is not registered in any side table, and whose
/// `addr - 8` word is readable — the probes that read a `GcHeader` (regex magic,
/// Date/Temporal brands) must be safe to call on arbitrary pointer-shaped
/// values, so the scratch address deliberately has readable bytes in front of
/// it rather than being a bare integer.
fn unregistered_scratch_addr() -> usize {
    let boxed: Box<[u64; 16]> = Box::new([0; 16]);
    let base = Box::into_raw(boxed) as usize; // leaked on purpose: process-lifetime
    base + 64
}

/// Every probe must answer "no" for an address nothing ever registered. This is
/// the fast path when the latch is idle and the ordinary table miss when it is
/// not, so the assertion holds in both states and the test is order-independent.
#[test]
fn unregistered_address_misses_every_probe() {
    let addr = unregistered_scratch_addr();

    assert_eq!(crate::typedarray::lookup_typed_array_kind(addr), None);
    assert!(!crate::buffer::is_registered_buffer(addr));
    assert!(!crate::buffer::is_uint8array_buffer(addr));
    assert!(!crate::buffer::is_array_buffer(addr));
    assert!(!crate::buffer::is_shared_array_buffer(addr));
    assert!(!crate::buffer::is_any_array_buffer(addr));
    assert!(!crate::buffer::is_data_view(addr));
    assert!(!crate::buffer::is_secret_key(addr));
    assert!(!crate::buffer::is_detached_buffer(addr));
    assert_eq!(crate::buffer::crypto_key_meta(addr), None);
    assert_eq!(crate::buffer::asymmetric_key_meta(addr), None);
    assert!(!crate::symbol::is_registered_symbol(addr));
    assert!(!crate::shared_sab::is_shared_sab(addr));
    assert!(
        !crate::regex::regexp_data_of(crate::value::js_nanbox_pointer((addr) as i64)).is_some()
    );
    assert!(!crate::map::is_registered_map(addr));
    assert!(!crate::set::is_registered_set(addr));
    assert!(!crate::object::is_registered_class_prototype_object(addr));
}

/// Constructing a typed array after a negative header probe must expose its
/// brand to instanceof, element-access and formatting paths immediately.
#[test]
fn typed_array_is_found_after_the_idle_fast_path_ran() {
    let scratch = unregistered_scratch_addr();
    // 1. take the probe's fast path at least once.
    assert_eq!(crate::typedarray::lookup_typed_array_kind(scratch), None);

    // 2. only now create the feature.
    let ta = crate::typedarray::js_typed_array_new(crate::typedarray::KIND_FLOAT64 as i32, 4.0);
    assert!(!ta.is_null(), "test premise: the typed array allocated");

    // 3. the probe must see it.
    assert_eq!(
        crate::typedarray::lookup_typed_array_kind(ta as usize),
        Some(crate::typedarray::KIND_FLOAT64),
        "a typed array created after the idle fast path must still be registered"
    );
    // The unrelated scratch address must NOT have become a typed array.
    assert_eq!(crate::typedarray::lookup_typed_array_kind(scratch), None);
}

#[test]
fn buffer_is_found_after_the_idle_fast_path_ran() {
    let scratch = unregistered_scratch_addr();
    assert!(!crate::buffer::is_registered_buffer(scratch));

    let buf = crate::buffer::buffer_alloc(32);
    assert!(!buf.is_null(), "test premise: the buffer allocated");

    assert!(
        crate::buffer::is_registered_buffer(buf as usize),
        "a Buffer created after the idle fast path must still be registered"
    );
    assert!(!crate::buffer::is_registered_buffer(scratch));
}

#[test]
fn uint8array_mark_is_found_after_the_idle_fast_path_ran() {
    let scratch = unregistered_scratch_addr();
    assert!(!crate::buffer::is_uint8array_buffer(scratch));

    let buf = crate::buffer::store::alloc_test(crate::gc::GC_TYPE_BUFFER_UINT8ARRAY, 8) as usize;

    assert!(
        crate::buffer::is_uint8array_buffer(buf),
        "`new Uint8Array(...)` identity must survive the idle fast path"
    );
    assert!(!crate::buffer::is_uint8array_buffer(scratch));
}

#[test]
fn array_buffer_and_data_view_marks_are_found_after_the_idle_fast_path_ran() {
    let scratch = unregistered_scratch_addr();
    assert!(!crate::buffer::is_array_buffer(scratch));
    assert!(!crate::buffer::is_data_view(scratch));

    let ab = crate::buffer::store::alloc_test(crate::gc::GC_TYPE_BUFFER_ARRAY_BUFFER, 16) as usize;

    let dv = (crate::buffer::bytes::from_slice(crate::buffer::bytes::Brand::DataView, &[0; 16])
        .to_bits()
        & crate::value::POINTER_MASK) as usize;

    assert!(crate::buffer::is_array_buffer(ab));
    assert!(crate::buffer::is_any_array_buffer(ab));
    assert!(crate::buffer::is_data_view(dv));
    assert!(!crate::buffer::is_array_buffer(scratch));
    assert!(!crate::buffer::is_data_view(scratch));
}

/// A `SharedArrayBuffer` backing is process-global and was never in any
/// thread-local table; its brand is its own header (#10694), so the probes
/// must answer "yes" for it on every thread.
#[test]
fn shared_array_buffer_backing_is_found_after_the_idle_fast_path_ran() {
    let scratch = unregistered_scratch_addr();
    assert!(!crate::buffer::is_registered_buffer(scratch));
    assert!(!crate::buffer::is_shared_array_buffer(scratch));

    let sab = crate::shared_sab::alloc_shared_sab(64) as usize;

    assert!(crate::shared_sab::is_shared_sab(sab));
    assert!(
        crate::buffer::is_registered_buffer(sab),
        "a SAB backing must read as a buffer from its own header"
    );
    assert!(crate::buffer::is_shared_array_buffer(sab));
    assert!(crate::buffer::is_any_array_buffer(sab));
    assert!(!crate::buffer::is_shared_array_buffer(scratch));
}

/// The cross-thread half of the SAB contract: a backing allocated on another
/// agent must be recognised here, from the header it was born with.
#[test]
fn shared_array_buffer_allocated_on_another_thread_is_found_here() {
    let store = std::thread::spawn(|| {
        let sab = crate::shared_sab::alloc_shared_sab(32) as usize;
        crate::shared_sab::shared_store_owner(sab).expect("sender store capability")
    })
    .join()
    .expect("SAB allocation thread");
    // Crossing agents shares the permanent store, then creates an agent-local
    // owner. The sender's local owner dies with that agent.
    let sab = crate::shared_sab::wrap_shared_sab(store) as usize;
    assert!(crate::shared_sab::is_shared_sab(sab));
    assert!(crate::buffer::is_registered_buffer(sab));
    assert!(crate::buffer::is_shared_array_buffer(sab));
}

#[test]
fn symbol_is_found_after_the_idle_fast_path_ran() {
    let scratch = unregistered_scratch_addr();
    assert!(!crate::symbol::is_registered_symbol(scratch));

    let sym = unsafe { crate::symbol::alloc_symbol(std::ptr::null_mut(), false) } as usize;
    assert!(sym != 0, "test premise: the symbol allocated");

    assert!(
        crate::symbol::is_registered_symbol(sym),
        "a Symbol created after the idle fast path must still be registered"
    );
    assert!(!crate::symbol::is_registered_symbol(scratch));
}

/// Map and Set already carried the #7474 latch; the contract is asserted here
/// alongside the rest so the whole family is covered by one test module.
#[test]
fn map_and_set_are_found_after_the_idle_fast_path_ran() {
    let scratch = unregistered_scratch_addr();
    assert!(!crate::map::is_registered_map(scratch));
    assert!(!crate::set::is_registered_set(scratch));

    let map = crate::map::js_map_alloc(4) as usize;
    let set = crate::set::js_set_alloc(4) as usize;

    assert!(crate::map::is_registered_map(map));
    assert!(crate::set::is_registered_set(set));
    assert!(!crate::map::is_registered_map(scratch));
    assert!(!crate::set::is_registered_set(scratch));
}

#[test]
fn detached_buffer_mark_is_found_after_the_idle_fast_path_ran() {
    let scratch = unregistered_scratch_addr();
    assert!(!crate::buffer::is_detached_buffer(scratch));

    let ab = crate::buffer::store::alloc_test(crate::gc::GC_TYPE_BUFFER_ARRAY_BUFFER, 16) as usize;

    assert!(!crate::buffer::is_detached_buffer(ab));
    crate::buffer::detach_array_buffer(ab);

    assert!(
        crate::buffer::is_detached_buffer(ab),
        "`ArrayBuffer.prototype.detached` must survive the idle fast path"
    );
    assert!(!crate::buffer::is_detached_buffer(scratch));
}

/// An address no allocator on any supported platform can return: above
/// `addr_class::is_valid_obj_ptr`'s heap ceiling, so no concurrently running
/// test can widen a registry window to cover it.
const FAR_OUTSIDE_ANY_WINDOW: usize = 0x7000_0000_0000_0000;

/// The symbol address FILTER. `is_registered_symbol` is asked about arbitrary
/// pointer-shaped values on the generic property, coercion and iteration paths,
/// and the answer is essentially always "no" — but a symbol the collector has
/// EVACUATED must keep answering "yes" from its new address, which is what the
/// forwarding rewrite's admission is for (see
/// `gc::tests::copying_side_tables::test_copying_minor_keeps_moved_symbol_visible_to_the_range_filter`).
///
/// A Bloom filter has false positives, so "the filter rejected this particular
/// address" is not by itself a proof that it can reject: the probe counter is,
/// and the second half — an address the filter must ADMIT — is what makes the
/// first half able to fail.
///
/// The worker below is a deterministic stand-in for an unrelated test: it
/// admits this exact address as a false positive in its own filter. Test builds
/// must isolate that filter alongside `SYMBOL_POINTERS`; a process-global
/// filter carries the worker's admission here and reproduces #9344.
#[test]
fn symbol_probe_rejects_a_filtered_address_without_touching_the_registry() {
    std::thread::spawn(|| {
        let unrelated_sym =
            unsafe { crate::symbol::alloc_symbol(std::ptr::null_mut(), false) } as usize;
        assert!(unrelated_sym != 0, "test premise: the symbol allocated");
        crate::symbol::admit_symbol_pointer(FAR_OUTSIDE_ANY_WINDOW);
    })
    .join()
    .expect("the unrelated symbol registration must finish");

    let before = crate::symbol::test_symbol_filter_admitted_probe_count();
    assert!(!crate::symbol::is_registered_symbol(FAR_OUTSIDE_ANY_WINDOW));
    assert_eq!(
        crate::symbol::test_symbol_filter_admitted_probe_count(),
        before,
        "the address filter must answer without reaching SYMBOL_POINTERS"
    );

    let sym = unsafe { crate::symbol::alloc_symbol(std::ptr::null_mut(), false) } as usize;
    assert!(sym != 0, "test premise: the symbol allocated");
    assert!(
        crate::symbol::is_registered_symbol(sym),
        "the filter must not hide a registered symbol"
    );
    assert!(
        crate::symbol::test_symbol_filter_admitted_probe_count() > before,
        "a filter-admitted address must reach SYMBOL_POINTERS"
    );
}

/// The class-prototype probe uses an exact inverse address index. Keep both
/// positive and negative membership covered: class-heavy bundles saturate the
/// older monotone filter, while this index must remain O(1) and exact (#9225).
#[test]
fn class_prototype_probe_uses_exact_inverse_membership() {
    use crate::object as class_registry;

    // A registered prototype, seeded through the real store so the inverse
    // index is updated exactly as production updates it.
    let proto = crate::object::js_object_alloc(0, 2) as usize;
    assert!(proto != 0, "test premise: the prototype object allocated");
    class_registry::test_seed_class_prototype_object_root(0x7f00_0001, proto);

    assert!(
        !class_registry::is_registered_class_prototype_object(FAR_OUTSIDE_ANY_WINDOW),
        "an address no registration admitted is not a class prototype"
    );
    assert!(
        class_registry::is_registered_class_prototype_object(proto),
        "the inverse index must find a registered class prototype"
    );

    // Two class ids can share one prototype. Replacing one must retain the
    // old address until the last forward-map reference moves away.
    let replacement = crate::object::js_object_alloc(0, 2) as usize;
    class_registry::test_seed_class_prototype_object_root(0x7f00_0002, proto);
    class_registry::test_seed_class_prototype_object_root(0x7f00_0001, replacement);
    assert!(class_registry::is_registered_class_prototype_object(proto));
    class_registry::test_seed_class_prototype_object_root(0x7f00_0002, replacement);
    assert!(!class_registry::is_registered_class_prototype_object(proto));
    assert!(class_registry::is_registered_class_prototype_object(
        replacement
    ));
}

/// The ordering rule itself, modelled on a private latch + table pair so both
/// orderings can be run. This is the "prove the gate can fail" half: if
/// arm-after-insert were harmless the wrong-order case would be indistinguishable
/// from the right one, and none of the comments in `registry_latch.rs` would be
/// worth writing.
mod latch_semantics {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static TABLE: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    }

    fn table_contains(addr: usize) -> bool {
        TABLE.with(|t| t.borrow().contains(&addr))
    }

    fn probe(latch: &RegistryLatch, addr: usize) -> bool {
        if latch.is_idle() {
            return false;
        }
        table_contains(addr)
    }

    /// The rule: arm, then publish.
    fn register_correctly(latch: &RegistryLatch, addr: usize, observe: &mut dyn FnMut()) {
        latch.arm();
        observe();
        TABLE.with(|t| t.borrow_mut().push(addr));
        observe();
    }

    /// The bug the rule exists to prevent: publish, then arm.
    fn register_wrongly(latch: &RegistryLatch, addr: usize, observe: &mut dyn FnMut()) {
        TABLE.with(|t| t.borrow_mut().push(addr));
        observe();
        latch.arm();
        observe();
    }

    #[test]
    fn arm_before_publish_is_never_observably_inconsistent() {
        let latch = RegistryLatch::new();
        let addr = 0xBEEF_0000usize;
        let mut inconsistent = false;
        {
            let mut observe = || {
                // The probe must never deny an entry the table already holds.
                if table_contains(addr) && !probe(&latch, addr) {
                    inconsistent = true;
                }
            };
            register_correctly(&latch, addr, &mut observe);
        }
        assert!(
            !inconsistent,
            "arm-before-publish must have no window in which the table holds \
             the entry and the probe still answers `false`"
        );
        assert!(probe(&latch, addr));
        TABLE.with(|t| t.borrow_mut().clear());
    }

    #[test]
    fn arm_after_publish_is_observably_inconsistent() {
        let latch = RegistryLatch::new();
        let addr = 0xFEED_0000usize;
        let mut inconsistent = false;
        {
            let mut observe = || {
                if table_contains(addr) && !probe(&latch, addr) {
                    inconsistent = true;
                }
            };
            register_wrongly(&latch, addr, &mut observe);
        }
        assert!(
            inconsistent,
            "sabotage check: publishing before arming MUST produce a window in \
             which a live entry reads as absent — if this stops failing, the \
             ordering rule has stopped being load-bearing and the check above \
             is proving nothing"
        );
        TABLE.with(|t| t.borrow_mut().clear());
    }

    #[test]
    fn latch_never_goes_back_to_idle() {
        let latch = RegistryLatch::new();
        assert!(latch.is_idle());
        latch.arm();
        for _ in 0..4 {
            latch.arm();
            assert!(!latch.is_idle());
        }
    }
}
