### Fixed

An array local captured by a nested closure read back `undefined` in its own
declaring scope, once `Array.prototype` had ever carried an indexed property.
The closure kept seeing the correct array, so the two storages for one binding
disagreed and `peek() === dest` was false. The trigger is rare — something must
put an indexed property on `Array.prototype`, which arms a monotone deopt latch
that deleting the property does not clear — but the affected shape, an array
local captured by a closure, is ordinary code, and the symptom is a silently
empty-looking variable rather than a crash.

`0ed806587c` (#10488) added a `ctx.local_types` refresh to the redeclaration
branch of `lower_let`, so `is_numeric_expr` and `static_type_of` would stop
disagreeing about a hoisted `var`. **It fixed that desync for hoisted `var` and
introduced a new one for box-captured locals.** A captured local reaches that
same branch without any redeclaration in the source: `Stmt::PreallocateBoxes`
registers the id up front, so its one real `Stmt::Let` finds `ctx.locals`
already populated and lands there. For such an id the refined type describes the
VALUE while the slot holds a box pointer, and the `local_types` readers then
lower reads as raw local loads instead of going through `js_box_get_bits`.

The fix skips the refresh when `ctx.boxed_vars` holds the id — an existing
`FnCtx` field already in scope, documented as the set whose `LocalGet` unboxes
through `js_box_get_bits`. A hoisted `var` is unboxed unless separately
captured, so #10488 keeps its fix; `test_gap_10488_var_array_void_compare` and
both `let_stmt_var_redeclare_tests` stay green.

A box-captured local now keeps whatever type the predefine recorded, normally
`Any`, so it can lose a fast path. That is the conservative direction — the cost
is a fast path, never correctness — and it is not a regression against any
working behaviour: before `0ed806587c` these locals got no refinement at all,
and after it they got one that described the value while the slot held a box
pointer. If refinement for captured locals is wanted later, the right shape is a
box-aware type, not this line.

Found via `test_gap_array_side_mask_covers_a_pointer_stored_at_a_late_index`,
which regressed in merge train 219. Despite its name and its companions living
in `gc/tests/`, it is not a GC bug: reading the slot *before* the `gc()` showed
it already `undefined`, which removed the collector in one probe. `slice`
passing while `splice` failed was an ordering artefact — the first invocation in
a process passes because the latch is not yet armed. Attributed by bisect over
six builds, each with its runtime stamp checked against the commit under test,
narrowing train 219 to `0ed806587c`. A plain revert was measured and rejected:
it fixes this bug but reds `test_gap_10488`, moving the failure rather than
clearing it.

Verified on one 871-fixture gap-suite invocation: 864 pass, 7 fail, 0 compile
fail, 0 crash. All 7 are pre-existing — the six standing `gap_snapshot.json`
entries in exactly their recorded state, plus `test_gap_9592_child_timeout_threads`
(#10730, a macOS fixture-portability bug). `test_gap_10488_var_array_void_compare`
(position 46), the new `test_gap_10727_captured_array_local_proto_index` (59) and
`test_gap_array_side_mask_...` (353) all passed in that same pass.

Note for the next change to `crates/perry-codegen/src/stmt/let_stmt.rs`: it is
now at exactly 2000 lines, the `check_file_size.sh` cap. It was at 1999 before
this change and the combined #10488/#10727 comment was condensed to fit the
guard in at net +1 line. Anything further needs the file split first.
