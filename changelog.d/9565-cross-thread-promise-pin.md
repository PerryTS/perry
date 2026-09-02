### Fixed

- **A promise handed to native code is now rooted until it settles; `fetch`
  (and ~110 other stdlib hand-offs) could be freed mid-flight (#9552).**
  `claude -p <120,000-char argument>` compiled with perry died with
  SIGSEGV in the microtask pump (`pump_protected`, `si_addr=0`) where node
  exits 1; nondeterministic, ~50% of runs.

  A promise minted by `js_promise_new_cross_thread` leaves the runtime as a
  bare `usize` inside a worker future, which no root scanner visits, and
  nothing on the JS side points **at** a pending promise whose only consumer
  is an `await` — `P.on_fulfilled` and `P.next` are edges out of it. The
  constructor's contract made the pin the caller's job; `spawn` and
  `Atomics.waitAsync` took it, the stdlib's `fetch`/db/ws sites never did.
  An old-generation reclaim at an allocation point ran its malloc sweep while
  `js_fetch_with_options`'s promise was in flight and freed it (never
  pinned, no token, still pending); mimalloc gave the 80-byte slot to a
  `RegExp`; the stdlib pump resolved the stale address and the pump then
  read `REGEXP_MAGIC` as the promise's `next`. The from-space quarantine
  reports it as an unrelated fault because the object was never in the
  arena. Diagnosed with a symbolized build and an env-gated trace of every
  promise allocation, malloc-sweep free, pin and token event.

  The constructor now owns the invariant: `js_promise_new_cross_thread`
  pins the (malloc-resident, non-moving) promise itself — one flag bit,
  through `pin_object_non_young` so the copying minor's young-pin latch is
  never armed — and `js_promise_resolve` / `js_promise_reject` release it
  with one byte test on a field in the padding after `state` (no other
  field moves; an arena promise pays a predictable-branch load and nothing
  else). `remove_token_from_registry` releases it too, so a native-async
  token dropped without settling cannot leak its promise. The caller-side
  pins in `spawn` and `waitAsync` are gone.

  Every place a raw promise address re-enters the runtime from native code
  — the stdlib pump, the native-async token pump, the `perry/thread`
  result drain — now classifies it (`native_promise_from_raw`) and aborts
  naming the site and the slot's current occupant, once per I/O completion,
  never per `await`. `scripts/check_cross_thread_promise_provenance.py` is
  a new `lint` step: it fails on an **arena** promise reaching a native
  settlement sink or a spawned closure (shadowing- and alias-aware,
  self-tested with three planted and three clean shapes). Unit coverage in
  `promise/cross_thread_pin_tests.rs` (pinned at creation, released by both
  settlements, survives `js_gc_collect()` while only an XOR-hidden integer
  holds it, token-drop release, address classification); gap test
  `test_gap_9552_cross_thread_promise_survives_gc.ts` fetches from a local
  server that answers late while collections are forced in between.

  Validation on the report's binary (`cli_2.1.112.js`, `--enable-wasm-runtime`,
  same compiler, only the runtime archives swapped): 4/5 crashes → 0/12.
