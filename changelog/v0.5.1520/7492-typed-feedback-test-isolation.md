### Test: `typed_feedback` order dependence was a poisoned mutex, not leaking codegen state (#7490)

`cargo test -p perry-codegen --test typed_feedback` failed 5 of 15 as a suite,
deterministically under `--test-threads=1` and with a wobbling victim set under
default parallelism. That reads like process-global codegen state leaking
between in-process compiles. It was not, and two of the five also fail when run
**alone** — the premise that all five pass in isolation does not hold.

Two assertions had drifted from intentional codegen changes.
`typed_feedback_guards_direct_class_field_specialization` matched the numeric
coercion in the *textual window* between the `class_field_get_number.fallback`
and `.merge` labels; #7430 split that arm, leaving `.fallback` holding only the
nullish-receiver check and moving the by-name load plus coercion into
`.fallback_lookup` — a block rendered **after** `.merge`, so the window could
never match again. `typed_feedback_trace_dump_runs_before_entry_return` cut
`main`'s body at the literal header `define i32 @main() {`, which stopped
matching when #7370 made native roots the default and every emitted function
gained `"frame-pointer"="non-leaf"`.

The first of those panics *while holding* the suite's `ENV_LOCK`. The unwind
poisons the mutex for the rest of the process, and every later
`ENV_LOCK.lock().unwrap()` then dies with `PoisonError` whatever its own
subject is doing. That is the entire "order dependence": one genuine failure,
three `PoisonError` casualties, and a victim set that reshuffles with the
scheduler because *which* tests reach the lock after the poisoning is a
scheduling fact. It is the test-harness cousin of the "a gate that cannot fail"
family — here, a gate that fails for a reason unrelated to what it measures.

Fixed by isolation plus two re-pointed assertions, neither loosened:

- `env_lock()` recovers a poisoned guard. Sound because each test declares its
  `EnvVarGuard` *after* the lock guard, so reverse drop order restores the env
  var during unwind before the mutex is released — the protected state is
  already consistent at poison time. One test's failure must fail that test
  alone.
- The class-field assertion now proves, end to end, the data flow the
  positional window stood in for: `.fallback_lookup` records the fallback call,
  loads by name and coerces; its terminator branches to the numeric merge; and
  the merge phi's fallback incoming **is** the coerced register. A `block_body()`
  helper reads a named block by stable label prefix, so per-function label
  suffixes and block ordering stop being load-bearing.
- `entry_fn_body` matches `main`'s exact signature and cuts at that line's
  opening brace, so unrelated function-attribute changes can no longer fail
  the test.
- `env_lock_is_poison_tolerant_so_one_failure_cannot_cascade` is a sabotage
  test for the isolation itself: it plants the exact #7490 shape, asserts the
  mutex really was poisoned (the gate asserts its subject was live), and then
  demands the accessor still hands out a guard. Against the pre-fix
  `.lock().unwrap()` it fails and reproduces the cascade at 9 of 16 red. It
  sorts first, so the whole suite afterwards runs under a genuinely poisoned
  lock.

No production change: `PERRY_TYPED_FEEDBACK` is read live at each call site and
`PERRY_FULL_OUTLINE_IC`'s decision is already a `thread_local!` set once per
`compile_module` — deliberately not a process-global `OnceLock`, so a
multi-module build cannot pin the first module's decision.

Validation: 16/16 green, three consecutive runs at default parallelism and
three at `--test-threads=1`, and every test also passes run alone.

The other suites named in the same sweep are a different root cause: every one
of their failures reproduces run alone, so none is order-dependent. They split
into the #7370 native-roots default (#7493 — `shadow_slot_hygiene` goes 0/12 to
11/12 under `PERRY_RS4GC=0`, while `native_proof_regressions` *loses* 13 tests
there, so the pin has to be per-test and `NativeRootsPin::shadow()` is
`#[cfg(test)]`) and four lowering-independent assertion failures (#7494).
