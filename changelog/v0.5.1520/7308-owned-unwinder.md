### Exception transport: owned single-phase unwinder (#7302 follow-up)

The `invoke`/`landingpad` migration (#7305) traded `longjmp`'s O(1) register
restore for real stack unwinding, which made throws slower — honestly reported
at the time. This closes that gap without giving back any of the correctness.

The system unwinder walks every frame **twice** (search + cleanup) and
re-decodes each frame's CFI on **every throw** (measured: 512 ns per
frame-step on macOS arm64). Perry needs neither property: the handler stack
already *is* the search result, and throw paths repeat, so decoded rows can be
cached. `js_throw` now walks to the handler itself with a per-PC row cache and
installs the handler frame's register context directly.

| microbenchmark (20k iters, macOS arm64) | system unwinder | owned walker | node/V8 |
|---|---|---|---|
| deep unwind, 200 frames | 4096 ms | **287 ms** (14.3×) | 168 ms |
| shallow throw + catch | 451 ms | **80 ms** (5.6×) | 110 ms |

Deep-unwind throws went from 24× slower than V8 to within 1.7×; shallow throws
now beat V8. Non-throwing paths are untouched.

**Safety.** Every register reload dereferences a computed address, so a
misdecoded row is a wild read rather than a wrong answer. Three layers, each
required to prove it ran:

- The owned walk reproduces `_Unwind_Backtrace`'s frame chain exactly (unit
  differential).
- `PERRY_EH_WALKER=diff` predicts (landing pad, CFA) before each raise and
  asserts it inside the personality against the system unwinder, tallying
  verified/declined at exit so a silent run cannot pass for a verified one:
  **20,000 deep unwinds (~4M frame steps), the GC throw-across-collection
  probe, and the smoke corpus — zero mispredictions, zero declines.** The
  checker's own liveness was proven by deliberately corrupting a prediction
  and confirming the abort fires.
- Stepping is fail-safe: the CFA must climb a plausible stack monotonically
  and every slot address must lie inside the walk's stack span, else the walk
  declines and the system unwinder carries that throw with identical
  semantics. Not theoretical — unguarded, the walker segfaulted stepping
  `libtest`'s frame shapes, which compiled programs never produce.

`d8..d15` are tracked and restored alongside the integer callee-saves: a
handler frame holding a live `f64` across the `try` would otherwise resume
with a stale value (silent numeric corruption, not a crash).

**Acceptance.** The full gap suite under the owned transport returns the
byte-identical failure set as merged main (95.4%, same 21 mismatches, same
single known crash). The GC probe passes under default,
`PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1`, and `PERRY_GEN_GC=0`.
`PERRY_EH_WALKER=off` reverts to the system unwinder for bisection (verified
live: the deep-unwind benchmark returns to 4231 ms).

**Platform scope.** aarch64/macOS takes the fast path; every other target
keeps the system unwinder unchanged — the walk simply declines when it has no
image to decode. Linux bring-up needs `dl_iterate_phdr` + `PT_GNU_EH_FRAME`
discovery; the stepping and cache above it are platform-independent.
