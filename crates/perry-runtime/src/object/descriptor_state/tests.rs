//! The young-owner log sabotage tests for `descriptor_state.rs`.

mod young_log_sabotage_tests {
    use super::super::*;

    #[test]
    fn descriptor_log_rederivation_rejects_a_suppressed_setter() {
        let _lock = crate::gc::global_side_table_test_lock();
        // An array owner: an ordinary object's data attributes live with its
        // keys (charter step 3) and never reach this log.
        let owner = crate::array::js_array_alloc(0) as usize;
        state().descriptors.young_owners.borrow_mut().clear();
        TEST_SUPPRESS_DESCRIPTOR_YOUNG_NOTE.with(|flag| flag.set(true));
        set_property_attrs(
            owner,
            "sabotage".to_string(),
            PropertyAttrs::new(true, true, true),
        );
        TEST_SUPPRESS_DESCRIPTOR_YOUNG_NOTE.with(|flag| flag.set(false));
        let missed = std::panic::catch_unwind(|| {
            state()
                .descriptors
                .young_owners
                .borrow()
                .debug_assert_logged(
                    DESCRIPTOR_YOUNG_LOG_NAME,
                    &relevant_descriptor_owners(state()),
                );
        });
        clear_property_attrs(owner, "sabotage");
        state().descriptors.young_owners.borrow_mut().clear();
        assert!(
            missed.is_err(),
            "sabotage: suppressing set_property_attrs' note must trip completeness"
        );
    }
}
