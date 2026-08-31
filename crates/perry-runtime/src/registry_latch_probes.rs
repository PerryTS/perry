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
    assert_eq!(crate::buffer::buffer_ab_alias(addr), None);
    assert!(!crate::symbol::is_registered_symbol(addr));
    assert!(!crate::shared_sab::is_shared_sab(addr));
    assert!(!crate::regex::is_registered_regex(addr));
    assert!(!crate::map::is_registered_map(addr));
    assert!(!crate::set::is_registered_set(addr));
    assert!(!crate::object::is_registered_class_prototype_object(addr));
}

/// #7474-shape regression: constructing a typed array AFTER the idle fast path
/// has already answered "not a typed array" must still register. A latch armed
/// after the registry insert — or a stale negative left in the `PERRY_TA_KIND_CACHE`
/// by the idle path — would make this array invisible to every `instanceof`,
/// element-access and formatting path.
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
    assert!(
        crate::typedarray::typed_array_registry_ever_used(),
        "registering a typed array must arm the latch"
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

    let buf = crate::buffer::buffer_alloc(8) as usize;
    crate::buffer::mark_as_uint8array(buf);

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

    let ab = crate::buffer::buffer_alloc(16) as usize;
    crate::buffer::mark_as_array_buffer(ab);
    let dv = crate::buffer::buffer_alloc(16) as usize;
    crate::buffer::mark_as_data_view(dv);

    assert!(crate::buffer::is_array_buffer(ab));
    assert!(crate::buffer::is_any_array_buffer(ab));
    assert!(crate::buffer::is_data_view(dv));
    assert!(!crate::buffer::is_array_buffer(scratch));
    assert!(!crate::buffer::is_data_view(scratch));
}

/// A `SharedArrayBuffer` backing is process-global and enters neither
/// thread-local registry, so it is the one case where a probe must answer "yes"
/// for an address the *local* tables have never seen. It therefore has to arm
/// `is_registered_buffer`'s latch as well as its own — an omission here would
/// leave every SAB invisible to `Buffer`/`Uint8Array` dispatch.
#[test]
fn shared_array_buffer_backing_is_found_after_the_idle_fast_path_ran() {
    let scratch = unregistered_scratch_addr();
    assert!(!crate::buffer::is_registered_buffer(scratch));
    assert!(!crate::buffer::is_shared_array_buffer(scratch));

    let sab = crate::shared_sab::alloc_shared_sab(64) as usize;

    assert!(crate::shared_sab::is_shared_sab(sab));
    assert!(
        crate::buffer::is_registered_buffer(sab),
        "a SAB backing must read as a registered buffer even though it never \
         enters BUFFER_REGISTRY — `alloc_shared_sab` arms that latch too"
    );
    assert!(crate::buffer::is_shared_array_buffer(sab));
    assert!(crate::buffer::is_any_array_buffer(sab));
    assert!(!crate::buffer::is_shared_array_buffer(scratch));
}

/// The cross-thread half of the SAB contract: a backing allocated on another
/// agent must be recognised here. The latch is process-global precisely so this
/// keeps working — a thread-local latch would let the receiving thread take its
/// own idle fast path and deny an address that is genuinely shared.
#[test]
fn shared_array_buffer_allocated_on_another_thread_is_found_here() {
    let sab = std::thread::spawn(|| crate::shared_sab::alloc_shared_sab(32) as usize)
        .join()
        .expect("SAB allocation thread");

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

    let ab = crate::buffer::buffer_alloc(16) as usize;
    crate::buffer::mark_as_array_buffer(ab);
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

/// The window is a fast path, so it must be shown to *run*, not merely to give
/// the right answer — a probe that reached the registries and missed returns
/// `false` too. The probe counter distinguishes the two, and the second half of
/// this test is what makes the first half able to fail: an always-`false`
/// `may_contain` would pass the rejection assertion and fail here.
#[test]
fn buffer_probe_rejects_an_out_of_window_address_without_touching_the_registries() {
    // Two real registrations, so the window has an interior rather than a
    // single point.
    let first = crate::buffer::buffer_alloc(32) as usize;
    let second = crate::buffer::buffer_alloc(32) as usize;
    let (lo, hi) = crate::buffer::test_buffer_addr_window_bounds()
        .expect("registering a buffer must open the address window");
    assert!(
        lo <= first.min(second) && hi >= first.max(second),
        "the window must cover every registered buffer: \
             [{lo:#x}, {hi:#x}] vs {first:#x} / {second:#x}"
    );

    let before = crate::buffer::test_buffer_registry_probe_count();
    assert!(
        !crate::buffer::is_registered_buffer(FAR_OUTSIDE_ANY_WINDOW),
        "an address outside the window is not a registered buffer"
    );
    assert_eq!(
        crate::buffer::test_buffer_registry_probe_count(),
        before,
        "the address window must answer without reaching the registries"
    );

    // A registered address must still be admitted AND still resolve — this is
    // the direction in which a wrong window is a type confusion, not a slowdown.
    assert!(
        crate::buffer::is_registered_buffer(first),
        "the window must not hide a registered buffer"
    );
    assert!(
        crate::buffer::test_buffer_registry_probe_count() > before,
        "an in-window address must reach the registries"
    );
}

#[test]
fn typed_array_probe_rejects_an_out_of_window_address_without_touching_the_registry() {
    let first = crate::typedarray::typed_array_alloc(crate::typedarray::KIND_UINT8, 4) as usize;
    let second = crate::typedarray::typed_array_alloc(crate::typedarray::KIND_FLOAT64, 4) as usize;
    let (lo, hi) = crate::typedarray::test_typed_array_addr_window_bounds()
        .expect("registering a typed array must open the address window");
    assert!(lo <= first.min(second) && hi >= first.max(second));

    let before = crate::typedarray::test_typed_array_window_admitted_probe_count();
    assert_eq!(
        crate::typedarray::lookup_typed_array_kind(FAR_OUTSIDE_ANY_WINDOW),
        None
    );
    assert_eq!(
        crate::typedarray::test_typed_array_window_admitted_probe_count(),
        before,
        "the address window must answer without reaching the registry \
         or writing a negative cache entry"
    );

    assert_eq!(
        crate::typedarray::lookup_typed_array_kind(first),
        Some(crate::typedarray::KIND_UINT8),
        "the window must not hide a registered typed array"
    );
    assert!(crate::typedarray::test_typed_array_window_admitted_probe_count() > before);
}

/// The Uint8Array-mark window, on the same terms as the buffer one above: it
/// must be shown to RUN, not merely to answer correctly, and it must not hide a
/// marked backing. `typed_array_owner_kind` asks this question on every untyped
/// element access, so the rejection is the common case by a wide margin.
#[test]
fn uint8array_probe_rejects_an_out_of_window_address_without_touching_the_registries() {
    let first = crate::buffer::buffer_alloc(32) as usize;
    crate::buffer::mark_as_uint8array(first);
    let second = crate::buffer::buffer_alloc(32) as usize;
    crate::buffer::mark_as_uint8array(second);
    let (lo, hi) = crate::buffer::test_uint8array_addr_window_bounds()
        .expect("marking a Uint8Array must open the address window");
    assert!(
        lo <= first.min(second) && hi >= first.max(second),
        "the window must cover every marked backing: \
             [{lo:#x}, {hi:#x}] vs {first:#x} / {second:#x}"
    );

    let before = crate::buffer::test_uint8array_registry_probe_count();
    assert!(
        !crate::buffer::is_uint8array_buffer(FAR_OUTSIDE_ANY_WINDOW),
        "an address outside the window is not a marked Uint8Array backing"
    );
    assert_eq!(
        crate::buffer::test_uint8array_registry_probe_count(),
        before,
        "the address window must answer without reaching the registries"
    );

    assert!(
        crate::buffer::is_uint8array_buffer(first),
        "the window must not hide a marked Uint8Array backing"
    );
    assert!(
        crate::buffer::test_uint8array_registry_probe_count() > before,
        "an in-window address must reach the registries"
    );
}

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
#[test]
fn symbol_probe_rejects_a_filtered_address_without_touching_the_registry() {
    let sym = unsafe { crate::symbol::alloc_symbol(std::ptr::null_mut(), false) } as usize;
    assert!(sym != 0, "test premise: the symbol allocated");

    let before = crate::symbol::test_symbol_filter_admitted_probe_count();
    assert!(!crate::symbol::is_registered_symbol(FAR_OUTSIDE_ANY_WINDOW));
    assert_eq!(
        crate::symbol::test_symbol_filter_admitted_probe_count(),
        before,
        "the address filter must answer without reaching SYMBOL_POINTERS"
    );

    assert!(
        crate::symbol::is_registered_symbol(sym),
        "the filter must not hide a registered symbol"
    );
    assert!(
        crate::symbol::test_symbol_filter_admitted_probe_count() > before,
        "a filter-admitted address must reach SYMBOL_POINTERS"
    );
}

/// The class-prototype address filter. The probe behind it is a LINEAR SCAN
/// (#9225) reached through a thread-local and an `RwLock`, and its one caller —
/// `descriptor_state::disable_inline_guards_for_descriptor_target` — runs on
/// every `Object.defineProperty`, so the rejection is what keeps a bundle's
/// `__export(exports, { … })` init off the scan entirely.
#[test]
fn class_prototype_probe_rejects_a_filtered_address_without_scanning() {
    use crate::object as class_registry;

    // A registered prototype, seeded through the real store so the filter is
    // admitted exactly as production admits it.
    let proto = crate::object::js_object_alloc(0, 2) as usize;
    assert!(proto != 0, "test premise: the prototype object allocated");
    class_registry::test_seed_class_prototype_object_root(0x7f00_0001, proto);

    let before = class_registry::test_class_prototype_scan_count();
    assert!(
        !class_registry::is_registered_class_prototype_object(FAR_OUTSIDE_ANY_WINDOW),
        "an address no registration admitted is not a class prototype"
    );
    assert_eq!(
        class_registry::test_class_prototype_scan_count(),
        before,
        "the address filter must answer without reaching the scan"
    );

    assert!(
        class_registry::is_registered_class_prototype_object(proto),
        "the filter must not hide a registered class prototype"
    );
    assert!(
        class_registry::test_class_prototype_scan_count() > before,
        "a filter-admitted address must reach the scan"
    );
}

/// `alloc_shared_sab` publishes a backing that `is_registered_buffer` reports
/// as a buffer without it ever entering `BUFFER_REGISTRY`, so the window has to
/// be widened on that route too. Calling the allocator directly (rather than
/// `js_shared_array_buffer_new`, which also calls `register_buffer`) is what
/// makes this test able to fail: it exercises the `note_buffer_like_registered`
/// path alone.
#[test]
fn shared_sab_backing_is_inside_the_buffer_address_window() {
    let sab = crate::shared_sab::alloc_shared_sab(64) as usize;
    assert!(
        crate::buffer::is_registered_buffer(sab),
        "a SharedArrayBuffer backing must stay visible to `is_registered_buffer`"
    );
    let (lo, hi) = crate::buffer::test_buffer_addr_window_bounds()
        .expect("a SAB allocation must open the address window");
    assert!(
        lo <= sab && sab <= hi,
        "the SAB route must widen the window: {sab:#x} outside [{lo:#x}, {hi:#x}]"
    );
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
