- ci/lint: repair three gates `main` left red after the Step 5 P4 shape-only GC flip.
  - `e2e-scoped` ("Compute e2e suite scope"): 7de3c1ad8 deleted
    `crates/perry-codegen/tests/typed_shape_declared_at_allocation.rs` together with
    the typed-layout descriptor tables it tested (`js_gc_declare_typed_shape_layout`
    is gone), so the suite was deleted, not renamed. Its `_CODEGEN_SUITES` entry in
    `scripts/ci_e2e_scope.py` is removed. Every other `crates/perry-codegen/tests/*.rs`
    suite is still mapped (`native_proof_support/` is a helper directory, mapped by its
    own path key).
  - `lint` "GC store-site inventory": 8a51a41b5 (retire object layout notes and header
    state) dropped three parameters from `emit_jsvalue_slot_store_pointer_tested`
    (12 -> 9 args) and two from `emit_static_store_ic_bookkeeping` (8 -> 6). The stems
    did not change: `class_field_set` is still emitted by `expr/property_set.rs` and
    `property_set/sloppy_class_field.rs`, and `put.pic` by `expr/put_value_store_ic.rs`
    and `stmt/region_loop/bare.rs`. The "stale registry entry" and "stale binding"
    errors were downstream of the arity miss (the census skipped those sites), so
    `STEM_EMITTER_ARG_INDEX` moves to 8 and 5 and no registry entry, probe or binding
    is deleted. The self-test fixtures follow the new arity, and two new self-test
    cases (V-P4b/V-P4c) assert that a call shorter or longer than the table expects
    still goes red; reverting the index to the old value turns the self-test red.
  - `lint` "Thread-exit custody of address-keyed globals": 7de3c1ad8 deleted
    `gc/shape_install.rs`, whose same-named thread-local `HITS`/`RECORDS` cells were
    the only non-counter mentions of those names. With them gone the rule discharges
    `object/field_get_set/for_in_stable.rs:HITS`, `object/inherited_read_cache.rs:HITS`,
    `object/prop_plan.rs:HITS` and `object/prop_plan.rs:RECORDS` mechanically, so their
    now-redundant inventory entries are deleted as the tool requires.
