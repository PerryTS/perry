Registered `DELETE_TRANSITION_TEST_OVERRIDE` in
`scripts/gc_runtime_root_holders.json` with a `test_only` verdict.

`gc_runtime_root_holders.py` enumerates every `static`/`thread_local!` whose
type could hold a heap pointer and requires a written verdict for any that a
registered scanner does not reach. The new flag is one, so the gate refused the
tree — the gate working.

The verdict is accurate rather than convenient: the declaration sits inside a
`#[cfg(test)] thread_local!` block and the type is `Cell<Option<bool>>`, which
cannot hold a heap pointer at all — `Option<bool>` is a two-bit value, not a
NaN box and not a `*mut`. There is nothing for a scanner to reach, and the
shipping path reads it and finds `None`.
