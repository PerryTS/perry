### `class X extends EventTarget` threw `TypeError: EventTarget is not a function` (#7518)

Fixes #7518 — a regression of #6301, which #6311 closed on 2026-07-13 and covered
with `test-files/test_gap_6301_event_target_subclass.ts`. That test had been red
since **c6ed8175d** ("fix(globals): complete Node parity", #6853, 2026-07-30) with
nothing to notice it: `parity` (which runs the gap suite) is gated to tag pushes,
so a gap regression lands on `main` without a red check.

**Bisect.** Predicate: compile and run `class Bus extends EventTarget {}; new
Bus()`, own `--profile perry-dev` build of `-p perry -p perry-runtime -p
perry-stdlib -p perry-runtime-static -p perry-stdlib-static` at every hop.
GOOD at `81ab8e5d6` (#6311, the commit that added the test) and at `17c0ff952`
(`c6ed8175d^`); BAD at `c6ed8175d` and at `main` (`33acba4c1`).

**Root cause.** One line of #6853 — adding `"EventTarget"` to
`builtin_constructor_spec_length` in
`crates/perry-runtime/src/object/global_this_tables.rs`, so `EventTarget.length`
reads `0` like Node — silently flipped a gate in another module.

`try_dispatch_value_called_proto_method`
(`crates/perry-runtime/src/object/native_call_method/proto_dispatch.rs`)
implements the #3716 uncurry-this idiom: a built-in *prototype method* invoked as
a value arrives backed by the shared `global_this_builtin_noop_thunk`, so the
helper recovers the closure's recorded `name` and re-dispatches
`IMPLICIT_THIS.<name>(…)` through the real by-name tower. Global *constructors*
share that same no-op thunk, and the only thing keeping them out was incidental —
its own doc comment said it was "gated on a recorded built-in `.length` so bare
no-op-backed global constructors … which never call `set_builtin_closure_length`,
are excluded". `populate_global_this_builtins` calls `set_builtin_closure_length`
for every name `builtin_constructor_spec_length` answers, so the moment
`EventTarget` joined that table the exclusion stopped holding for it. It was never
an invariant, just an incomplete table.

The break: `class Bus extends EventTarget {}` has no static parent class id, so
HIR captures the heritage as a dynamic `extends_expr` and `lower_call/new.rs`
emits `js_fetch_or_value_super(<globalThis.EventTarget>, this, …)`. That helper
binds `IMPLICIT_THIS` to the new instance and calls `js_native_call_value`, which
— gate now open — re-dispatched `bus.EventTarget()`; the by-name tower's catch-all
threw. Before #6853 the same call reached the no-op thunk, returned `undefined`,
and `super()` was the intended best-effort no-op.

The same latent hole applied to every other no-op-thunk-backed global constructor
carrying a spec `.length` (`AbortController`, `AbortSignal`, `FormData`,
`TextEncoder`, `DisposableStack`, `URLSearchParams`, …); `EventTarget` is simply
the one with a gap test. The `Function | GeneratorFunction |
AsyncGeneratorFunction` special case already in the same function (#5588) is an
earlier instance of exactly this bug, patched name-by-name.

**Fix.** Make the exclusion explicit and table-driven: decline any name in
`GLOBAL_THIS_BUILTIN_CONSTRUCTORS` (new `is_global_this_builtin_constructor_name`),
which subsumes the #5588 special case. A global constructor invoked as a value is
never a prototype-method uncurry, so re-dispatching it by name on the receiver is
always wrong. Global builtin *functions* (`parseInt`, `fetch`, `structuredClone`,
…) are untouched: they are installed with their own thunks, never the no-op one,
so they never reach the helper.

**Regression proofing.** `crates/perry-runtime/src/object/tests.rs` gains
`global_builtin_constructor_values_are_not_redispatched_by_name`, which walks the
whole `GLOBAL_THIS_BUILTIN_CONSTRUCTORS` table and asserts the helper declines
every entry, so a future `builtin_constructor_spec_length` addition cannot
re-open the hole for a different name. It asserts its own subject was live (it
fails if no entry still reaches the no-op-thunk-plus-recorded-`.length` shape the
bug needs) and pins that the fix is the exclusion, not dropping the Node-parity
`.length`. Reverting the production change fails it on all ~70 entries. It runs in
the per-PR `cargo-test` tier rather than the tag-gated gap suite.

**Validation.** `test_gap_6301_event_target_subclass` byte-identical to
`node --experimental-strip-types` (Node 26.5.1), 37 lines, exit 0. Untouched
neighbours all still byte-identical: `test_gap_3716_uncurry_this` (the idiom this
helper exists for), `test_issue_2142_builtin_proto_method_values`,
`test_issue_3579_function_call_apply_eval`, `test_gap_global_builtins_2905_2889`,
`test_globalthis_builtins`, `test_gap_new_globalthis_builtin_6726`,
`test_gap_globals_abortsignal_navigator_2582_2923`,
`test_gap_escape_unescape_global_4511`. A before/after A/B on the same host
confirms the change is behaviour-neutral outside the bug: `const S = Symbol;
S("x")` and `AbortController.length` read the same (pre-existing gaps, unrelated
to this path), while `class X extends AbortController | FormData | TextEncoder |
DisposableStack | URLSearchParams {}` stops throwing (their instance surface is a
separate, older gap).
