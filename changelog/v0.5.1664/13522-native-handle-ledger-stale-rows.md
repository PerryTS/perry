Re-lock the native-handle ledger after two table removals, and drop the stale `fn_metadata` classifications that were failing the gate's own self-test.

`scripts/native_handle_ledger.txt` still budgeted three L1 tables to `crates/perry-runtime/src/node_submodules/mod.rs`, but `97e40e8826` replaced those three address-keyed maps (`EXPORT_SINGLETONS`, `NAMESPACE_SINGLETONS`, `DEFAULT_OBJECT_SINGLETONS`, each a `RefCell<HashMap<_, *mut …Header>>`) with per-submodule slots behind a single pointer. The file declares no handle table today, so the row was stale and the gate failed on it.

The same regeneration lowers `crates/perry-stdlib/src/worker_threads.rs` from 5 to 3. That reduction was checked against its commit rather than taken from `--update`: `cc1da0e683` deleted `UNTRANSFERABLE_OBJECTS` and `UNCLONEABLE_OBJECTS`, two `RefCell<HashSet<u64>>` handle-id sets, which is exactly two L1 tables. Both ledger changes only lower ceilings, as the file's header requires.

`scripts/native_handle_ledger.py` then failed its own self-test on a second and independent defect: `NON_HANDLE_TABLES` still classified `builtins/fn_metadata.rs` `REGISTRY` and `OVERRIDES` as non-handle tables, but `e91af6efe6` replaced both modules' tables with stable function-metadata identity, and the file now declares no `static` at all. Both entries are gone.

Verified in both directions. As committed, the gate and its self-test exit 0 and the ceiling totals match the tree, so the `debt fell` advisory is silent. Driven deliberately, the guards still fire: re-adding one classification makes the self-test exit 1 with `stale classification entries`, and deleting a live ledger row makes the gate exit 1 with `tables 3 exceeds ceiling 0`.
