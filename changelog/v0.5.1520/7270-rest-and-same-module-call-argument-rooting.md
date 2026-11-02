### Fixed

- **`codegen`: the rest-argument and same-module direct-call paths now root
  their arguments too** (#7154). #7240 fixed `lower_call/extern_func.rs`'s
  cross-module NON-rest arm and named two siblings it would not ship
  unmeasured. These are those two, each with a gap test that is a hard fault on
  the parent.

  **`extern_func.rs`'s `has_rest` arm** had *two* unprotected registers where
  the non-rest arm had one. The fixed parameters, as before — except their
  window does not close when the last argument is lowered, because the rest
  array is materialized afterwards and materializing it runs `js_array_alloc`
  plus one `js_array_push_f64` per trailing argument. And the **accumulator**,
  which has no analogue in the non-rest arm: `current` is a raw
  `*mut ArrayHeader` in a bare SSA register, threaded through the push loop,
  holding the only reference to every argument pushed so far while the next
  argument's expression is lowered. Nothing rooted it, so a minor landing in
  that window was free to *sweep* the array, not merely move it.
  `temp_root::rooted_array_begin` has named this exact shape as "the shape
  behind every variadic / spread / rest argument list" since #6951 and
  `console_promise.rs` has used it since; this path never adopted it.

  **`func_ref.rs`'s same-module arms** — all four — had the identical defect.
  #7240's regression test needed a two-file fixture precisely because a
  same-file callee does not reach `extern_func.rs` at all: it resolves through
  `Expr::FuncRef(fid)` into `func_ref.rs`, so the bug sat one `else` away,
  unreached by that PR's test. This was not folded into #7240 because
  `func_ref.rs` threads its lowered arguments through four specialized-ABI
  dispatch paths, each a fast/fallback diamond with a phi at the merge; the
  temp-root release has to sit in the merge block that post-dominates all five
  call sites, since releasing on one side of a diamond leaves the other side's
  call reading dropped slots.

  All five arms now share one `lower_call/mod.rs` helper rather than five
  copies of the same three mistakes. Each argument is still gated by
  `temp_root::operand_protection`, so a list of scalars emits the IR it emitted
  before.

  **The `func_ref.rs` release is emitted after `implicit_this_restore`, not
  before**, and the order is load-bearing. `implicit_this_save` (#7211) runs
  *below* the argument lowering, so its slot sits *above* this group, and
  `js_gc_temp_root_truncate` drops `base` and everything above it. Releasing
  first therefore drops the saved receiver, and `js_gc_temp_root_get` answers an
  out-of-range read with `0` — so the restore would rebind the enclosing
  method's `this` to the *number* `0`. That is a miscompile, not a rooting bug,
  and it fires whenever a same-module callee reads dynamic `this` and at least
  one argument takes a real slot.

  Measured, per gap test, compiled **and** run with
  `PERRY_GC_MOVING_LOOP_POLLS=1`:

  | | parent (`6aeef5baf`) | this branch |
  |---|---|---|
  | polls only | `bad 0` 10/10 | `bad 0` 10/10 |
  | polls + `PERRY_GC_ZEAL=1` | **0/10 — SIGSEGV every run** | `bad 0` **10/10** |
  | polls + zeal + `PERRY_GEN_GC=0` | `bad 0` 10/10 | `bad 0` 10/10 |

  The first row is why both test files carry a `parity-env:` line. Without it
  the harness runs them in the default configuration, the broken compiler prints
  `bad 0`, and the files gate nothing: polls are off by default since #7161, so
  the IR has no back-edge safepoint to collect on, and without zeal the only
  collections are allocation-triggered, which take
  `ManualGcScanGuard::force_full_scan` and make the copying minor ineligible —
  nothing moves, so a stale register still names a live object.
  `run_parity_tests.sh` applies `parity-env` to the perry compile *and* the
  perry run, which is what `PERRY_GC_MOVING_LOOP_POLLS` needs, since it is read
  at both. The `PERRY_GEN_GC=0` row is the control that proves the tests track
  collector mode rather than being flaky.

  `sfw-registry --help`, `PERRY_FORCE_WELL_KNOWN=iovalkey`, compiled and run
  with `PERRY_GC_MOVING_LOOP_POLLS=1`, same runtime archives and same routing
  decisions on both arms: unchanged at **30/30**. This PR is not measured as a
  registry improvement — #7240 already took that workload to 30/30, and the
  point here is that these three edits do not give it back.

### Added

- `test-files/test_gap_gc_rest_argument_rooting.ts` (+ its cross-module fixture
  `test-files/fixtures/gc_call_arg_rooting_pkg/rest_callee.ts`) and
  `test-files/test_gap_gc_same_module_call_argument_rooting.ts`. The rest test
  needs its own fixture file rather than reusing `callee.ts`, because the arm is
  chosen by the *callee's* signature: `joinArgs` has no rest param and takes the
  arm #7240 fixed, `joinRest` has one and takes the arm pinned here. The
  same-module test needs no fixture at all, for the same reason in reverse — a
  same-file callee is what routes the call into `func_ref.rs`. Both exercise
  both protections: a string-literal argument is `OperandProtection::Reload`, a
  local holding a freshly-allocated string is `OperandProtection::Root`.

### Changed

- **`scripts/gc_root_dominance_check.py` models a load of a string-literal
  handle global as a heap-value source** (#7154). `--stale-registers`
  classified a source as an `ALLOC_RE` call or a shadow-slot load; a
  `load double, ptr @…_.str.N.handle` is neither, so the register it defines
  was never tracked and no stale use could be attributed to it. That is the
  blind spot #7240 shipped its fix through, and #7240's own writeup says so:
  "the register it defines is never tracked as a heap value and no stale use
  can be attributed to it". Demonstrated below rather than asserted — on the
  parent's IR the widening reports 48 `--moving-only` uses the parent checker
  reports zero of, and every one is a literal argument at the faulting call.

  The pattern already existed: `--unrooted-allocas` had `REWRITTEN_LOAD_RE` and
  used it, while `--stale-registers` had only `GLOBAL_ROOT_RE` and knew about
  `@perry_global_*` alone. **The two modes disagreed about what a
  collector-rewritten load is, and the narrower one was wrong.** There is now
  one definition and both modes read it. Unlike #7226's `js_implicit_this_set`
  and #7227's `js_regexp_new`, this could not be closed by adding a name to
  `ALLOC_RE` — the source is a `load`, not a `call`.

  Strictly additive by construction: `GLOBAL_ROOT_RE` is still consulted first,
  so no previously reported source changes kind.

  Measured over the 116-source / 136-module corpus
  (`scripts/gc_root_dominance_corpus.sh`), emitted twice — once by the parent
  compiler and once by this branch's — so the checker delta and the codegen
  delta can be read separately. Columns are the checker; rows are the compiler
  that emitted the IR.

  | corpus emitted by | mode | parent checker | this checker |
  |---|---|---|---|
  | parent codegen | `--stale-registers` | 2914 | **4805** (+1891 `strhandle`) |
  | parent codegen | `--moving-only` | 110 | **158** (+48 `strhandle`) |
  | parent codegen | `--moving-only --fatal-sinks` | 32 | 32 |
  | this codegen | `--stale-registers` | 2858 | **4693** (+1835 `strhandle`) |
  | this codegen | `--moving-only` | 62 | **62** (+0) |
  | this codegen | `--moving-only --fatal-sinks` | 0 | 0 |

  Read the two middle rows together, because that is the whole result. On the
  parent's IR the widening exposes 48 stale uses that reach a moving minor, and
  **all 48 are in the two gap tests added here** — every one is a
  `load double, ptr @…_.str.N.handle` feeding `joinRest` or `joinSameRest`
  below the rest-array construction, which is precisely the defect the codegen
  half of this PR fixes. There are none anywhere else in the corpus. On this
  branch's IR the same widening adds **zero** `--moving-only` uses. So the
  modelling is not too broad: it found one population, that population was
  real, and it is now empty.

  The codegen change reads out of the same table down the parent-checker
  column, which is an apples-to-apples measurement of the fix alone:
  `--moving-only` 110 → 62, and `--moving-only --fatal-sinks` **32 → 0**. Those
  32 were all `source=alloc sink=js_array_push_f64` — the unrooted rest
  accumulator, reported as an allocation held across the next `push`.

  The gate itself is untouched: `gc-root-dominance.yml` runs the bind-anchored
  mode, not `--stale-registers`, and exits 0 with 0 violations and 40/40 seeded
  violations caught on both corpora with both checkers. `--self-test` asserts
  the new source in both directions and under `--moving-only`, so the widening
  cannot silently stop working.

  One caveat recorded rather than hidden: the shared `REWRITTEN_LOAD_RE` also
  names `@perry_class_keys_*`, which `--unrooted-allocas` has always used. It
  contributes **0** hits in `--stale-registers` over this corpus, so that arm is
  currently carried by the shared definition rather than exercised by it.
