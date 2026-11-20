### Fix `loop_safepoint_purity`, red on `main` since the moving-GC work

Six of the seven tests in `crates/perry-codegen/tests/loop_safepoint_purity.rs`
were failing on `main`. Every assertion in that file is about *which* loops keep
their back-edge `js_gc_loop_safepoint()` poll, but the poll is gated behind
`PERRY_GC_MOVING_LOOP_POLLS`, which defaults off — so the emitter returned
before the purity analysis ran and the six "sabotage" cases (a call in the body,
an object literal, string concatenation, a module-global accumulator, coercible
bound/accumulator) saw no poll for a reason unrelated to what they test. The one
test asserting a *pure* numeric loop drops its poll passed for the same wrong
reason.

The suite now enables the knob it asserts on, before the first codegen call —
`moving_safepoint_polls_enabled` caches in a `OnceLock`.

Worth noting how it stayed red: integration suites under `crates/*/tests/` do
not run per-PR (nightly/tag only), which CLAUDE.md already calls out as a way a
regression "can land green and sit red for days".

This makes the suite exercise the poll-ON path. The **default** path — polls
off, which is what ships — remains untested, and `PERRY_GC_MOVING_LOOP_POLLS`
is a GC knob with no CI arm covering either state. That is a kill-policy
question for the owner, not something this change decides.
