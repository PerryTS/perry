### Fixed

- **GC: the timer family's callback and trailing arguments are rooted across their own argument list (#7210).**
  `setTimeout(cb, 0, {…}, churn())` had two unrooted windows, and the callback's
  was a live deterministic crash on `main`, not the staleness the issue predicted.

  `cb_box` was lowered first and read at `js_timer_validate_callback` — after the
  delay's `lower_expr` and after every trailing argument's. A freshly-allocated
  closure therefore sat in a bare SSA register across a user call carrying loop
  back-edge polls, and the moving minor inside it left the register naming
  from-space: `TypeError [ERR_INVALID_ARG_TYPE]: The "callback" argument must be
  of type function. Received an instance of Object`, 3/3 under
  `PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_HEAP_LIMIT=8` and identically on the
  `evac_minor` arm.

  The trailing-argument staging buffer is the second window and is worse in kind:
  argument *i* sits in a bare `alloca_entry_array`, storage the precise root walk
  never visits, while argument *i+1* is lowered. Nothing anywhere refers to the
  object at that moment, so it is a premature **sweep**, not a stale address.

  The `timers`-namespace forms carried a third: `cb_handle` was `unbox_to_i64`'d
  — a raw heap address, not even NaN-boxed — above the trailing arguments.

  All three close with one change: lower the whole argument list through
  `expr::temp_root::lower_exprs_rooted`, then fill the buffer in a second,
  lowering-free pass, and release the guard only after the consuming call (which
  reads the buffer). Cost is zero when nothing in the list can collect
  (`OperandProtection::Reuse`) — `setTimeout(fn, 0, someLocal)` emits
  byte-identical IR. Covers global `setTimeout`/`setInterval`/`setImmediate`,
  their `timers`-namespace siblings, and `process.nextTick`.

  `crates/perry-codegen/src/lower_call/extern_func.rs` crossed the 2000-line cap,
  so the timer arms moved to a new `lower_call/extern_timers.rs` — a pure
  mechanical move, dispatched immediately above the match so arm order is
  unchanged. Timer routing after the split was re-verified against node 26.5.1 on
  a program exercising every moved arm.

  Witness `test-files/test_gap_gc_staging_args_rooting.ts`, registered in
  `test-parity/gc_repsel_corpus.txt`: red 3/3 at base, `bad 0` 5/5 after, clean
  3/3 on the shipped default, byte-exact against node.

### Notes

- The `#7210` triage that motivated this change found that the **66**
  moving-reachable `gc_root_dominance_check.py --unrooted-allocas` reports are
  **all false positives**, and says so rather than adding roots: 64 are the
  `@perry_class_keys_*` pointer cache, whose array is allocated by
  `js_array_alloc_with_length_longlived` into the old arena and is therefore
  moved only by old-page defrag — which is **off by default** since #6206
  (`PERRY_GC_OLD_DEFRAG=1` opt-in); the other 2 are `js_box_alloc_bits` results,
  which come from `std::alloc::alloc`, are never freed and are never relocated
  (`scan_box_roots_mut` rewrites the JSValue *inside* the box, not its address).
  The checker's `_is_heap_source` conflates "a location the collector rewrites"
  with "an object the collector can move"; the sites it names are the former and
  not the latter.

- **GC: `setInterval`'s trailing arguments are now scanned (#7210, runtime half).**
  `scan_timer_roots_mut` walked `timer.args` for `CALLBACK_TIMERS` and for both
  `MOCK_TIMERS` lists, and never for `INTERVAL_TIMERS`; the incremental twin
  `scan_interval_timers_step` had the same hole, and cycle-based collections run
  **only** the step scanner. So `setInterval(fn, delay, { … })` left the object
  in a table nothing scanned — swept at the first collection, then handed to the
  callback as a dangling pointer on the next tick.

  This is a different failure class from a stale register: it does not need a
  collection to land in a narrow window, it goes wrong at collection #0 and
  stays wrong, and no static IR checker can see it (the table is not in the
  emitted IR). A partially-correct scanner is worse than an absent one, because
  it reads as covered.

  It is also the other half of the codegen fix above — rooting an argument
  across its own lowering buys nothing if the table it then lands in is not a
  root. Witness `test-files/test_gap_gc_interval_args_rooting.ts`, registered in
  the corpus: `BAD interval.a`/`BAD interval.b` from tick 1, 3/3 at base under
  `PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_HEAP_LIMIT=8`; `bad 0` 5/5 after; clean
  3/3 on the shipped default; byte-exact against node 26.5.1.

  The wider population this came from is enumerated in #7231.
