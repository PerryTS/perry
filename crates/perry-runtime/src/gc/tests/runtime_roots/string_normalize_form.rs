use super::*;
use crate::arena::FromSpaceProtection;

/// #8426: `String.prototype.normalize` borrowed the subject string's inline
/// WTF-8 payload BEFORE coercing its `form` argument, then read that borrow
/// after the coercion had returned.
///
/// The coercion is a collection point twice over: an inline short-string form
/// materializes onto the heap (so even `s.normalize("NFC")` allocates there),
/// and an object form runs user `toString`, whose loop back-edge polls run a
/// moving minor. Either can evacuate a young subject, and a `&str` taken
/// beforehand is a copy the collector cannot rewrite — rooting rewrites slots,
/// never already-materialized borrows. The normalization pass then read
/// retired from-space.
///
/// The test drives the object-form window, which is the one reachable from
/// user code today: `toString` forces a real copying minor. Three assertions
/// together, because any one alone can pass vacuously — (1) the subject was
/// young, (2) the collection actually MOVED it, so the window was live, and
/// (3) the normalized bytes are the subject's, not the retired page's.
///
/// `PoisonOnly` makes failure certain rather than lucky. Without it, whether a
/// stale borrow is *detected* depends on what the allocator happened to
/// recycle into the retired page; with it, those bytes are guaranteed poison.
/// The cost is the failure mode: a regression faults inside the normalization
/// pass (poison is not valid WTF-8) rather than reaching the byte assertions
/// below, so a reintroduced #8426 shows up as a SIGSEGV naming this test.
/// That is deliberate — a gate that can pass vacuously is not a gate.
#[test]
fn normalize_form_coercion_must_not_strand_the_subject_payload() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _mode = crate::arena::ProtectionModeGuard::set(FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();

    // Decomposed "café-normalize": NFC must compose e+U+0301 into U+00E9, so a
    // pass-through cannot be mistaken for a correct normalization.
    const SUBJECT: &[u8] = "cafe\u{301}-normalize".as_bytes();
    const EXPECTED: &[u8] = "caf\u{e9}-normalize".as_bytes();

    let scope = RuntimeHandleScope::new();
    let subject = crate::string::js_string_from_bytes(SUBJECT.as_ptr(), SUBJECT.len() as u32);
    // (1) premise: a heap string in the MOVABLE nursery. A `+=` accumulator
    // buffer or a large string lives outside it and would never relocate,
    // which would make every assertion below vacuous.
    assert!(
        crate::arena::pointer_in_nursery(subject as usize),
        "test premise: the subject must be nursery-resident, or nothing moves"
    );
    let subject_handle = scope.root_string_ptr(subject);
    let before_addr = subject as usize;

    // `{ toString() { <forces a copying minor>; return "NFC" } }`
    let form = crate::object::js_object_alloc(0, 1);
    let form_handle = scope.root_raw_mut_ptr(form);
    let to_string = crate::closure::js_closure_alloc(normalize_form_force_minor_gc as *const u8, 0);
    let to_string_handle = scope.root_raw_mut_ptr(to_string);
    let key = crate::string::js_string_from_bytes(b"toString".as_ptr(), 8);
    let key_handle = scope.root_string_ptr(key);
    form_handle.with_mut_ptr::<crate::object::ObjectHeader, _>(|form_ptr| {
        key_handle.with_const_ptr::<crate::StringHeader, _>(|key_ptr| {
            crate::object::js_object_set_field_by_name(
                form_ptr,
                key_ptr,
                to_string_handle.with_mut_ptr::<crate::closure::ClosureHeader, _>(
                    |to_string_ptr| crate::value::js_nanbox_pointer(to_string_ptr as i64),
                ),
            );
        });
    });

    NORMALIZE_FORM_COERCIONS.with(|c| c.set(0));
    let before_collections = gc_collection_count();
    let form_value = form_handle.with_mut_ptr::<crate::object::ObjectHeader, _>(|form_ptr| {
        crate::value::js_nanbox_pointer(form_ptr as i64)
    });
    // Two combinators, no bare read (#7341): `with_const_ptr` hands the
    // subject to `js_string_normalize`, which since the fix roots it itself
    // (a self-rooting entry point), and `across_const` hands back its
    // POST-collection address — the coercion inside moves it.
    let (result, after_ptr) = subject_handle.across_const::<crate::StringHeader, _>(|| {
        subject_handle.with_const_ptr::<crate::StringHeader, _>(|s| {
            crate::string::js_string_normalize(s, form_value)
        })
    });

    assert_eq!(
        NORMALIZE_FORM_COERCIONS.with(|c| c.get()),
        1,
        "the form's toString must have run exactly once"
    );
    assert!(
        gc_collection_count() > before_collections,
        "test premise: the coercion must have collected"
    );
    // (2) the subject really was evacuated inside the window — otherwise a
    // stale borrow would still point at live bytes and this test could not
    // distinguish the fix from the bug.
    let after_addr = after_ptr as usize;
    assert_ne!(
        after_addr, before_addr,
        "test premise: the copying minor must have MOVED the subject"
    );

    // (3) the normalization read the subject's live bytes, not the retired page.
    unsafe {
        assert_eq!(
            (*result).byte_len as usize,
            EXPECTED.len(),
            "normalized length must come from the live subject"
        );
        let data = (result as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, EXPECTED.len());
        assert_eq!(
            bytes, EXPECTED,
            "normalized bytes must be the subject's, not retired from-space"
        );
    }
}

thread_local! {
    static NORMALIZE_FORM_COERCIONS: Cell<u32> = const { Cell::new(0) };
}

/// The form object's `toString`: forces a real copying minor — the moving
/// collection a user `toString`'s loop back-edge polls would run — then
/// returns the form name.
extern "C" fn normalize_form_force_minor_gc(_closure: *const crate::closure::ClosureHeader) -> f64 {
    NORMALIZE_FORM_COERCIONS.with(|c| c.set(c.get() + 1));
    let _ = crate::gc::gc_collect_minor();
    test_string_value(b"NFC")
}
