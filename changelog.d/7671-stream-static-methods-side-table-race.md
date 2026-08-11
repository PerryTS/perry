**`fix(test-isolation)`: the stream constructor tests read a side table the GC guards clear.**

`stream_constructors_expose_static_method_values` asserts a static method value is not `TAG_UNDEFINED`, and intermittently it was — `assertion left != right failed, left: 9222246136947933185, right: 9222246136947933185`, i.e. both sides `0x7FFC_0000_0000_0001`.

`CLOSURE_PROPS` is a **process-global `Mutex<HashMap<usize, HashMap<String, f64>>>`** keyed by closure address, and the GC test guards' state reset (`test_clear_closure_side_tables`) clears it from whatever thread runs them. `gc/tests/support.rs` documents this in its own comment, and three tests in `closure/dynamic_props.rs` already take `crate::gc::global_side_table_test_lock()` for exactly this reason. The two `native_module_stream` tests did not.

0/16 on `main`, 1/12 on #7664's branch — the third time in two days that a branch has *exposed* a pre-existing global-sink race by changing the parallel schedule rather than introducing one (#7665 fixed the other two: `opt_report` and `ext_registry`). Verified 0/20 after.

**The class is wider than this fix.** A survey of files that read closure dynamic props in tests:

| guarded | file |
|---|---|
| 3 of 4 | `closure/dynamic_props.rs` |
| 2 of 9 | `object/global_this_webassembly.rs` |
| **0** | `array/tests.rs` (65), `node_stream_tests.rs` (42), `node_submodules/tests.rs` (33), `object/instanceof.rs` (5), `value/to_string.rs` (4), `object/native_module/constants.rs` (4), and ~14 more |

Blanket-locking several hundred tests would serialise a large part of the suite for a hazard that only bites a test reading a *persisted* prop across a window, so this change fixes the observed instance and the exposure is filed with the survey rather than guessed at.
