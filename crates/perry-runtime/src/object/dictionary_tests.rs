//! Unit pins for dictionary mode (#10868 step 2.5 stage 1).
//!
//! The ORDER semantics are pinned differentially against node by
//! `tests/parity/test_parity_dictionary_mode_order.ts`; what lives here is
//! what a differential test cannot see: the shape-identity namespace, the
//! mint COUNT the mode exists to bound, and the latch instrument's three
//! states.
//!
//! The GC edge has its own pin next to its sibling:
//! `gc::tests::dead_owner_side_tables::test_object_meta_dictionary_keys_survive_copied_minor_move`.

use super::dictionary;
use super::{js_object_alloc, js_object_get_field_by_name, js_object_set_field_by_name};

/// Restores the latch arming on scope exit (panic included) so a failing test
/// cannot leak its arming into unrelated tests on the same process.
///
/// It RESTORES WHAT IT FOUND. It used to disarm unconditionally, which was
/// the same thing while the default was off — and became the opposite of
/// restoring once #10868 armed the latch by default: every dictionary test
/// then leaked a DISARMED latch forward, and
/// `own_key_membership_crosses_65536_without_a_cutoff`, which runs later in
/// the same binary and whose key list is unique to it, hit the k(k+1)/2
/// cliff and was OOM-killed. It passed standalone and died in the suite,
/// which is the signature of leaked process-global state (L16.11).
fn scopeguard_latch() -> impl Drop {
    struct Restore(Option<u64>);
    impl Drop for Restore {
        fn drop(&mut self) {
            dictionary::test_arm_latch(self.0);
            dictionary::test_clear_layout_id_budget();
        }
    }
    Restore(dictionary::test_latch_state())
}

unsafe fn set_key(obj: *mut super::ObjectHeader, name: &str, value: f64) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
}

unsafe fn get_key(obj: *mut super::ObjectHeader, name: &str) -> f64 {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let bits = js_object_get_field_by_name(obj, key);
    f64::from_bits(bits.bits())
}

/// The three semantic-generation namespaces must be pairwise disjoint, or two
/// receivers that are semantically different can be handed one ShapeId.
///
/// `SHAPE_SEMANTIC_NEXT` draws have bit 63 clear and the counter aborts far
/// below 2^62; `deterministic_semantic_generation` sets bit 63 (pinned by
/// `tombstone_tests::the_delete_successor_generation_is_deterministic_not_a_counter_draw`);
/// dictionary draws set bit 62 and clear bit 63.
#[test]
fn dictionary_generation_namespaces_are_disjoint() {
    // Named *_BIT, not DETERMINISTIC: `global_sink_isolation.py` resolves an
    // asserted identifier by NAME across the crate with no scope awareness,
    // so a test-local const sharing a name with `stub_diag.rs`'s real
    // `static DETERMINISTIC` is reported as a new asserted process-global.
    const DETERMINISTIC_BIT: u64 = 1 << 63;
    assert_eq!(
        dictionary::DICTIONARY_GENERATION_TAG & DETERMINISTIC_BIT,
        0,
        "a dictionary generation must not land in the deterministic namespace"
    );
    assert_ne!(
        dictionary::DICTIONARY_GENERATION_TAG,
        0,
        "a dictionary generation must be distinguishable from a counter draw"
    );
    // A counter draw that reached bit 62 would collide. `alloc_shape_id` parks
    // at 2^30 ids and fail-stops long before the generation counter could get
    // within thirty-two orders of magnitude of this, but state the bound
    // rather than trust the comment.
    assert!(
        dictionary::DICTIONARY_GENERATION_TAG > (1u64 << 40),
        "the dictionary namespace must sit far above any reachable counter draw"
    );
}

/// The latch converts, and the conversion is visible in exactly the three
/// places it should be: the shape publishes no keys, the meta record carries
/// the list, and `object_keys_array` still answers with it.
#[test]
fn the_latch_moves_the_key_list_into_the_meta_record() {
    let _global = crate::gc::global_side_table_test_lock();
    let _restore = scopeguard_latch();
    unsafe {
        let obj = js_object_alloc(0, 0);
        for i in 0..6 {
            set_key(obj, &format!("dictlatch_{i:02}"), i as f64);
        }
        assert!(
            !dictionary::is_dictionary(obj),
            "test premise: the latch is off by default"
        );
        let before = super::object_keys_array(obj);
        assert!(!before.is_null(), "test premise: the receiver has keys");

        assert!(
            dictionary::latch_object_to_dictionary(obj),
            "the latch must convert an ordinary shaped receiver"
        );

        assert!(dictionary::is_dictionary(obj));
        let descriptor =
            super::shapes::object_shape_descriptor(obj).expect("a dictionary receiver is stamped");
        assert_eq!(descriptor.keys, 0, "the shape must publish no keys");
        assert_eq!(descriptor.logical_key_count, 0);
        assert_ne!(
            descriptor.semantic_generation & dictionary::DICTIONARY_GENERATION_TAG,
            0,
            "the dictionary shape must draw from the dictionary namespace"
        );

        let after = super::object_keys_array(obj);
        assert!(!after.is_null(), "the key list must still be reachable");
        assert_ne!(
            after, before,
            "the latch must take a PRIVATE copy: the source may be shared with \
             every sibling of its layout, and a dictionary receiver mutates \
             its array in place"
        );
        assert_eq!(crate::array::js_array_length(after), 6);

        // Values do not move: the mode relocates names, not values.
        for i in 0..6 {
            assert_eq!(get_key(obj, &format!("dictlatch_{i:02}")), i as f64);
        }
        // And it keeps working across the boundary.
        for i in 6..14 {
            set_key(obj, &format!("dictlatch_{i:02}"), i as f64);
        }
        for i in 0..14 {
            assert_eq!(
                get_key(obj, &format!("dictlatch_{i:02}")),
                i as f64,
                "key {i} read back wrong after the latch"
            );
        }
        assert_eq!(
            crate::array::js_array_length(super::object_keys_array(obj)),
            14
        );
    }
}

/// **P1, and the must-fail control.** The property the mode exists for:
/// appends to a latched receiver stop minting shape identities.
///
/// Measured on this lane's OWN counters rather than on the global
/// `SHAPE_ID_NEXT`. That counter is process-wide and every test running
/// beside this one draws from it, so a bound written against it is a bound on
/// the whole suite's concurrency, not on this object — it would be flaky in
/// one direction and vacuous in the other. `publications` counts the
/// key-list republications this receiver absorbed (one per append) and
/// `regenerations` counts the ones that still had to draw an identity, so the
/// ratio is exactly the claim and nothing else can move it.
///
/// The global counter still appears once, as the PREMISE: an ordinary
/// receiver really does mint per append. Without that the comparison would
/// be against an unknown.
///
/// **Must-fail control:** neuter the latch — make `latch_object_to_dictionary`
/// return `false`, or make `is_dictionary` return `false` — and `publications`
/// goes to 0 while the receiver mints per append exactly like the control.
/// A latch whose removal changes nothing is not a latch.
#[test]
fn appends_after_the_latch_mint_no_shape_ids() {
    let _global = crate::gc::global_side_table_test_lock();
    let _restore = scopeguard_latch();
    const APPENDS: u32 = 24;
    unsafe {
        // Premise: an ordinary receiver mints at least one id per append.
        let control = js_object_alloc(0, 0);
        for i in 0..4 {
            set_key(control, &format!("dictctl_{i:02}"), i as f64);
        }
        let before = super::shapes::test_shape_id_counter();
        for i in 4..(4 + APPENDS) {
            set_key(control, &format!("dictctl_{i:02}"), i as f64);
        }
        let unlatched = super::shapes::test_shape_id_counter() - before;
        assert!(
            unlatched >= APPENDS,
            "test premise: an ordinary receiver mints at least one id per \
             append (saw {unlatched} over {APPENDS}); without it the bound \
             below is vacuous"
        );

        // The claim.
        let obj = js_object_alloc(0, 0);
        for i in 0..4 {
            set_key(obj, &format!("dictarm_{i:02}"), i as f64);
        }
        assert!(dictionary::latch_object_to_dictionary(obj));
        dictionary::test_reset_counters();
        for i in 4..(4 + APPENDS) {
            set_key(obj, &format!("dictarm_{i:02}"), i as f64);
        }
        let publications = dictionary::dictionary_publications();
        let regenerations = dictionary::dictionary_regenerations();

        assert!(
            publications >= u64::from(APPENDS),
            "test premise: every append must reach `publish_keys` (saw \
             {publications} for {APPENDS} appends). Zero here is what the \
             must-fail control produces, so a green run with zero would mean \
             the test had stopped testing."
        );
        // A same-array append mints nothing; only a reallocation does, and
        // `js_array_push` grows geometrically. What must not survive is
        // linearity.
        assert!(
            regenerations * 4 < publications,
            "a latched receiver must draw asymptotically fewer identities \
             than it takes appends ({regenerations} draws for {publications} \
             appends). Equal counts mean the latch is doing nothing."
        );
        // Every append still landed, and reads still resolve.
        for i in 0..(4 + APPENDS) {
            assert_eq!(get_key(obj, &format!("dictarm_{i:02}")), i as f64);
        }
    }
}

/// **P5.** The instrument must distinguish "off", "armed and never reached"
/// and "armed, reached, declined" — the false-zero rule. A counter that only
/// says `0` cannot tell an operator which of the three happened.
#[test]
fn the_latch_counters_distinguish_never_fired_from_never_armed() {
    let _global = crate::gc::global_side_table_test_lock();
    let _restore = scopeguard_latch();
    unsafe {
        // Off: the predicate answers false and records nothing.
        dictionary::test_arm_latch(None);
        dictionary::test_reset_counters();
        assert!(!dictionary::dictionary_latch_armed());
        let obj = js_object_alloc(0, 0);
        for i in 0..4 {
            set_key(obj, &format!("dictcnt_{i:02}"), i as f64);
        }
        assert_eq!(
            dictionary::dictionary_latch_candidates(),
            0,
            "an unarmed latch must not even count: a default-off knob costs \
             one relaxed load and nothing else"
        );
        assert_eq!(dictionary::dictionary_latches(), 0);

        // Armed above every key count this loop reaches: REACHED and DECLINED.
        dictionary::test_arm_latch(Some(1_000_000));
        dictionary::test_reset_counters();
        let declined = js_object_alloc(0, 0);
        for i in 0..4 {
            set_key(declined, &format!("dictdec_{i:02}"), i as f64);
        }
        assert!(
            dictionary::dictionary_latch_candidates() > 0,
            "armed with candidates=0 is the bug shape: the knob is set and the \
             call site is not on the path"
        );
        assert_eq!(
            dictionary::dictionary_latches(),
            0,
            "the threshold was above every key count in this loop"
        );
        assert!(!dictionary::is_dictionary(declined));

        // Armed at a reachable threshold: FIRED.
        dictionary::test_arm_latch(Some(3));
        dictionary::test_reset_counters();
        let fired = js_object_alloc(0, 0);
        for i in 0..6 {
            set_key(fired, &format!("dictfire_{i:02}"), i as f64);
        }
        assert!(
            dictionary::dictionary_latches() > 0,
            "a latch that cannot be observed to fire is documentation"
        );
        assert!(
            dictionary::is_dictionary(fired),
            "the receiver that tripped the threshold must be in dictionary mode"
        );
        for i in 0..6 {
            assert_eq!(get_key(fired, &format!("dictfire_{i:02}")), i as f64);
        }
        assert!(
            dictionary::dictionary_publications() > 0,
            "the publications counter is what says the mode absorbed work \
             that would otherwise have minted"
        );
    }
}

/// A receiver carrying tombstones is refused, because `hole_count` is a fact a
/// keyless dictionary shape does not carry and latching over one would drop
/// it. Stated as a test so the limit is a decision, not an accident.
#[test]
fn a_receiver_with_holes_is_refused() {
    let _global = crate::gc::global_side_table_test_lock();
    let _restore = scopeguard_latch();
    unsafe {
        let obj = js_object_alloc(0, 0);
        for i in 0..20 {
            set_key(obj, &format!("dicthole_{i:02}"), i as f64);
        }
        // Two deletes: the first transfers ownership of a shared keys array,
        // only the second can tombstone (see `tombstone_tests`).
        for name in ["dicthole_11", "dicthole_07"] {
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
                as *const crate::StringHeader;
            super::delete_rest::js_object_delete_field(obj, key);
        }
        if super::shapes::object_shape_hole_count(obj) == 0 {
            // The tombstone lane is flag-gated; without a hole there is
            // nothing for this test to assert and it must not pretend.
            return;
        }
        assert!(
            !dictionary::latch_object_to_dictionary(obj),
            "a receiver with tombstones must be refused, not silently latched \
             with its hole count dropped"
        );
    }
}

/// **Trigger 2.** Layout-id exhaustion is a correctness trigger, not a policy
/// one: an object the interning allocator cannot give a layout id to cannot be
/// interned at all, so dictionary mode is the only place left for it.
///
/// A fixture that really exhausts a 24-bit id space is impractical, so the
/// budget is injected — which is the whole reason it is a published number
/// rather than an internal counter. Without this the exhaustion arm would be
/// a branch no test can reach, which is the shape of check that cannot fail.
#[test]
fn layout_id_exhaustion_latches_whatever_the_key_count() {
    let _global = crate::gc::global_side_table_test_lock();
    let _restore = scopeguard_latch();
    unsafe {
        // A budget that is merely LOW must not latch anything: the trigger is
        // exhaustion, not pressure. Without this half the test would pass on a
        // predicate that latched on any published budget at all.
        dictionary::test_arm_latch(None);
        dictionary::test_clear_layout_id_budget();
        dictionary::note_layout_id_budget(1);
        assert!(!dictionary::dictionary_layout_ids_exhausted());
        let spare = js_object_alloc(0, 0);
        set_key(spare, "dictx_low", 1.0);
        assert!(
            !dictionary::is_dictionary(spare),
            "a low budget is not an exhausted one"
        );

        // Exhausted: the next publication latches, whatever the key count,
        // and is attributed to trigger 2.
        dictionary::test_reset_counters();
        let obj = js_object_alloc(0, 0);
        set_key(obj, "dictx_a", 1.0);
        set_key(obj, "dictx_b", 2.0);
        assert!(
            !dictionary::is_dictionary(obj),
            "test premise: nothing has latched while ids are available"
        );

        dictionary::note_layout_id_budget(0);
        assert!(dictionary::dictionary_layout_ids_exhausted());
        set_key(obj, "dictx_c", 3.0);

        // Each step separately, so a failure names which one broke rather
        // than leaving "it did not latch" to be bisected by hand.
        assert!(
            dictionary::dictionary_latch_candidates() > 0,
            "the predicate was never reached: the latch site is not on the \
             add-key path"
        );
        assert!(
            dictionary::dictionary_exhaustion_latches() > 0,
            "the predicate was reached but did not attribute to exhaustion"
        );
        assert!(
            dictionary::dictionary_latches() > 0,
            "the predicate said yes and the conversion refused"
        );
        assert!(
            dictionary::is_dictionary(obj),
            "the conversion reported success and the receiver is not in \
             dictionary mode"
        );
        // And it is a working object, not just a converted one.
        set_key(obj, "dictx_d", 4.0);
        assert_eq!(get_key(obj, "dictx_a"), 1.0);
        assert_eq!(get_key(obj, "dictx_b"), 2.0);
        assert_eq!(get_key(obj, "dictx_c"), 3.0);
        assert_eq!(get_key(obj, "dictx_d"), 4.0);
    }
}
