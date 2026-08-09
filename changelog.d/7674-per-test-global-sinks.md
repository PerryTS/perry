**Test isolation: the GC test guards' state reset can no longer reach another test's data (#7672).**

`gc::tests::support::reset_copying_nursery_runtime_test_state()` runs from
`GcTestIsolationGuard` and `CopyingNurseryTestGuard`, on whatever libtest thread
constructs one, and calls 20 `test_clear_*` helpers that empty **process-global**
side tables. The guards serialize against each other and against the handful of
tests that remember to take `crate::gc::global_side_table_test_lock()`. Nothing
requires a *reader* to take it, so the defence was opt-in and the opt-in was
invisible at the read site.

Three flakes in two days, each exposed by an unrelated PR that changed the
parallel schedule and each diagnosed from the wrong VALUE rather than the timing:

| fixed in | table | presented as |
|---|---|---|
| #7665 | `opt_report`'s row sink | `rows.len() == 2` failing at **3** |
| #7665 | `ext_registry`'s `USED_PROVIDERS` | "empty" failing with `ioredis` present |
| #7671 | `closure`'s `CLOSURE_PROPS` | a static method read back as `TAG_UNDEFINED` |

### The fix is storage, not a lock

A lock cannot close this. The damage window is "between this test's write and
this test's read", and only the test knows that span — an accessor that takes a
shared lock for one call does not cover it, and a lock that does have to be taken
by the test, which is the opt-in the class is made of, on a reader population the
issue counts at ~180 and growing.

`per_test_global!` expands to the plain `static` outside a test build (byte
for byte — `test_clear_*` is `#[cfg(test)]` and never runs in a shipped runtime)
and to a per-thread instance inside one. libtest runs one thread per test, so
"per thread" and "per test" coincide, and a guard on thread U empties U's
instance, which already holds only U's entries — exactly the isolation the clear
was reaching for. Call sites are unchanged: `PerThread<T>` derefs to `T`, so
`SYMBOL_REGISTRY.lock()` and `CLASS_PROTOTYPE_OBJECTS.read()` keep working as
written, and this is a declaration-site change rather than a 300-site rewrite.

**26 tables converted**, across every family the guards clear: closure dynamic
props (3), symbol side tables (5), the class-registry `RwLock`s (7), the timer
queues (3), the object-constant caches and `GLOBAL_THIS_PTR` (9), `geisterhand`
(2), `ui_text_registry` (2), and the `console.log` singleton.

### Both halves are shown able to fail

`scripts/global_sink_isolation.py` runs in `lint` (a required context, ~1s, no
compiler). It derives the clear list from the reset function's own source,
follows one level of same-file accessors — `test_clear_closure_side_tables` names
no static at all, it goes through `get_closure_props()`, and a body-only scan
would have classified it "no storage" — resolves each identifier in its own
module first (`REGISTRY` exists in three files), and fails on any bare `static`
not allowlisted with an issue. An allowlist entry that matches nothing fails too,
so a fix must delete its line. `--self-test` has 9 checks, including running the
parsers against the real tree so a regex that stops matching cannot go green.

`gc::tests::global_sink_isolation` plants the #7671 shape per clear helper —
write on this thread, run the guards' real clear on another, read back — without
taking the global lock, because taking it would test the opt-in rather than the
isolation. Each probe asserts its subject was live before the clear. A canary
declared as a plain `static` runs the identical procedure and **requires** the
wipe to be observed.

Reverting the macro's `#[cfg(test)]` arm to the pre-fix bare static — one edit,
all 23 tables — fails **7 of the 9 tests**, each naming its table.

### Validation

25 consecutive `cargo test -p perry-runtime --lib --no-fail-fast` runs, 25 green,
0 vacuous (every run checked for `Running unittests` and for `error[` before its
result was counted): 1926 passed, 0 failed, ~8.5s each. `cargo check
--all-targets` clean; all 24 `lint` commands green.

`timer.rs` sat three lines under the 2000-line cap, so the macro takes an
optional trailing `;` and the three timer tables are declared on one line each.

### Surveyed, not fixed here

The same architecture, with the same split-lock-domain signature, exists off the
guards' clear path: `async_hooks`' `HOOKS`/`RESOURCES`/`NEXT_ASYNC_ID` under four
disjoint domains (and `gc/tests/alloc.rs:836` under none), `tui::state::SLOTS`
cleared under three different locks, and `agent_dispatch_tests.rs`'s private
`TIMER_QUEUE_TESTS` lock over the timer queues the guards also clear.

### Two more found by soaking the fix, and the macro renamed for what it does

A 22-run soak produced **two** reds, both the same class, neither reached by any
`test_clear_*` helper — so the clear list could not have found them, and neither
could a gate derived only from it:

* **`gc::barrier::GENERATED_WRITE_BARRIERS_EMITTED`** is owned by TWO guards
  under TWO DIFFERENT locks — `CopyingNurseryTestGuard` under the
  copying-nursery isolation lock, `GeneratedWriteBarrierTestGuard` under
  `GENERATED_BARRIER_TEST_LOCK` — and every runtime write barrier reads it
  holding neither. `sabotaged_parent_gate_strands_a_young_child_the_shipped_gate_keeps`
  failed with `missing_edges=1 ... slot_page_ever_dirty=false`: the barrier did
  not fire, because another thread's guard had zeroed the flag mid-test.
* **`tui::tree`'s `NEXT_HANDLE` / `REGISTRY`** have no clear and no lock, and
  `register_increments_handle` asserts `h2 == h1 + 1` plus an exact registry
  length. Any concurrent `register()` breaks both.

Both were diagnosed from the wrong VALUE, not the timing — a non-sequential
handle, and a barrier that did not dirty a page.

The macro is therefore named `per_test_global!`, for what it does, rather than
`guard_cleared_global!`, for the sharpest instance of what it defends against;
two of its residents are not cleared by anything. The gate now audits the
guards' own module alongside the clear list, which is what makes the
write-barrier flag reachable by it at all.

### Two ways this gate could have gone quiet, both closed

Found while extending it, and both now self-tested:

* The `per_test_global!(...)` **paren form** — used in `timer.rs` to stay under
  the 2000-line cap — was invisible to a `{`-only matcher, which silently took
  the three timer tables to "(no static storage)". A gate that stops matching
  reports zero hazards and exits 0, which is indistinguishable from a clean tree.
* A **classified-statics floor** now fires when the matchers stop matching, so a
  rotted regex fails loudly rather than passing vacuously. 93 classify today.

A `Mutex<()>` serializer is classified as a lock and is never a hazard: making
one per-thread would turn it into a no-op, which is the opposite of the fix.
Reverting the delimiter fix fails the paren-form self-test case; reverting the
floor fails its own.
