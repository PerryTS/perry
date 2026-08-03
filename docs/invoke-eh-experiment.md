# Exception lowering: setjmp/longjmp → LLVM `invoke`/`landingpad`

Status: **complete** — landed via PR #7305. The setjmp path and the temporary
`PERRY_EH` development flag are deleted; invoke/landingpad (SEH funclets on
windows-msvc) is Perry's only exception lowering. This document is the
campaign record: design decisions, the measurement matrices behind them, and
the acceptance evidence.

## Why

Perry lowers `try`/`catch` to `setjmp`/`longjmp` (`perry-codegen/src/stmt/try_stmt.rs`).
That one choice causes three separate problems:

1. **Precise moving-GC roots are unsound in `try` functions.** A `longjmp` can
   jump past a `gc.statepoint`'s `gc.relocate`, so the relocated pointer's
   write-back never runs. `exp/stackmap-viability` therefore excludes `has_try`
   functions from statepoints and routes them to a plain-stack-map lowering
   that is itself unsound (root slots recorded as caller-saved registers,
   3/60 locations on one probe). Under RS4GC it is worse: `mem2reg` cannot
   promote the volatile allocas setjmp needs, so try-region roots never enter
   SSA and never join a `gc-live` bundle.
2. **~570 lines of machinery exist only to fight the register allocator**:
   `volatile_setjmp.rs` (376) + `setjmp_abi.rs` (193) implement C99 7.13.2.1p3
   (values modified between `setjmp` and `longjmp` must be `volatile`).
3. **Every `try` function is pessimized**: `returns_twice` on the setjmp call
   plus `#1` (`noinline`) on the whole function are optimization barriers.

The `invoke`/`landingpad` form makes the unwind edge explicit in the IR:
relocations exist on both the normal and unwind edges, no jump can skip a
write-back, and none of the volatile/noinline machinery is needed.

## Phase 0 — spike results (macOS arm64, 2026-08-03)

Standalone probe: hand-written LLVM IR (invoke + landingpad + catch-all) linked
against a small Rust staticlib whose throw path is `_Unwind_RaiseException`.
All scenarios were run under both `panic=unwind` and `panic=abort` builds of
the Rust side.

### Which personality function?

**A Perry-specific `perry_eh_personality`**, implemented in `perry-runtime` as
a port of the standard Itanium LSDA walk (same shape as Rust std's
`rust_eh_personality`, which the spike used successfully as a stand-in — it is
class-agnostic and handles `catch ptr null` landing pads for a foreign
exception class). Owning the personality:

- avoids linking libc++abi (`__gxx_personality_v0`) into every produced binary
  and avoids `__cxa_begin_catch`'s foreign-exception edge cases;
- avoids depending on the unstable `rust_eh_personality` symbol's contract;
- is required anyway for the Windows SEH variant, which cannot use the
  Itanium personality at all.

### How does the thrown value map onto the landing pad's `{ ptr, i32 }`?

**It doesn't need to.** The landing pad ignores both slots. The thrown JS
value stays where it lives today: the GC-rooted TLS `current_exception` slot,
read by `js_get_exception()` / cleared by `js_clear_exception()` — the catch
blocks keep their exact current shape. The `_Unwind_Exception` object is a
per-thread static with class `PERRYJS\0` and a no-op cleanup fn; it carries no
payload. Bit-exactness of NaN-boxed payloads was verified through a full
throw/catch round trip (`0x7ffd000000123456` in → identical bits out).

### What does `js_throw` become?

Unchanged until its final line. It still: stores the value into the rooted TLS
slot, checks for the uncaught case, applies the async-context deferred
restores, and restores the shadow-stack / runtime-handle / method-depth /
prototype-resolution / dyn-eval savepoints for the target handler. Then,
instead of `longjmp`:

- if the innermost open handler is a **generated `try`** → `_Unwind_RaiseException`
  on the per-thread exception object. If that returns (`_URC_END_OF_STACK`),
  no landing pad existed — report uncaught and exit(1), as today.
- if the innermost open handler is a **Rust-side `js_call_catching` frame** →
  `longjmp`, exactly as today. Rust cannot catch a foreign exception
  (`catch_unwind` aborts on foreign classes), so the runtime-internal boundary
  trap keeps its private `ffi::setjmp`. This is not a second lowering for JS
  `try` — no generated code ever emits a setjmp again — it is the JS↔Rust
  boundary guard, and it is sound for the same reason it is sound today: the
  frames between the throw and the `js_call_catching` frame are *discarded*,
  never resumed, and an open `js_call_catching` handler is always innermost
  when it is the target (stack order mirrors handler-stack order), so a raise
  never crosses an open `js_call_catching` frame.

Rethrow (`finally` re-raise, catch-with-finally fail path) raises a fresh
exception via `js_throw`; the per-thread object is reusable because the
previous unwind completed when control reached the landing pad. `resume` is
never emitted.

**Key lowering rule confirmed by the spike:** a rethrow inside a landing-pad
successor must itself be an `invoke` wired to the *enclosing* handler's
landing pad — a plain `call` there sails past every handler in the same
function (the IP is outside all of the LSDA's invoke ranges). In general every
potentially-throwing call must carry the unwind label of the innermost
lexically-enclosing active handler, including inside catch and finally bodies.

### Phase 2 (answered early): can a throw cross runtime Rust frames?

Measured, all on the probe (extern "C" helper → interior call → JS callback →
throw; landing pad on the far side of the helper):

| Rust build | Result |
|---|---|
| `panic=unwind`, helper has an interior Rust call | **process abort** — rustc's abort-on-unwind guard (RFC 2945) fires on the Rust-ABI call site inside the `extern "C"` fn |
| `panic=unwind`, helper calls back through `extern "C"` sites only | caught (no guard on the active path) |
| `panic=unwind`, helper + callback typed `extern "C-unwind"` | caught, and the helper's `Drop` guards **run** during unwind |
| `panic=abort`, default flags | **uncaught / stranded** — rustc omits unwind tables, `_Unwind_RaiseException` cannot step the frame and returns `_URC_END_OF_STACK` |
| `panic=abort` + `-C force-unwind-tables=yes` | **caught, `Drop`s skipped** — exact longjmp-equivalent semantics |

Decision: **the runtime linked into produced binaries must be built
`panic=abort` with `-C force-unwind-tables=yes`.**

- It is the only configuration with longjmp-identical semantics, which keeps
  `js_throw`'s existing at-throw savepoint restores exactly correct (no Rust
  cleanups run behind them — the reason the C-unwind route is dangerous: with
  cleanups running *after* the at-throw restore, every skipped guard's `Drop`
  would double-restore counters, so all restores would have to move to the
  catch side).
- The mass `extern "C-unwind"` alternative also fails closed the wrong way: a
  single missed annotation is a production abort discovered only when a throw
  first crosses that helper, and it only works under `panic=unwind` (under
  `panic=abort`, a C-unwind fn that unwinds aborts by spec — also measured).
- Precedent: the auto-opt library builder already ships feature-stripped
  runtimes with `panic=abort` when no `catch_unwind` callers are present
  (`perry/src/commands/compile/optimized_libs/driver.rs`).
- Cost: `catch_unwind`-based panic recovery in `perry-runtime/src/thread.rs`
  (spawn-worker Rust panics → rejected promise) and
  `perry-stdlib/src/worker_threads.rs` stops catching — a runtime *bug* that
  panics becomes an abort instead of a rejection. JS exceptions are unaffected
  (they never used the panic mechanism). `cargo test` is unaffected (cargo
  forces unwind for test builds).
- Enforcement concern (the "gate must assert its subject is live" rule):
  `-C force-unwind-tables` rides on RUSTFLAGS/config, and a stray user
  `RUSTFLAGS` would silently drop it, stranding every cross-helper throw.
  The landed version must carry a self-check (see Phase 1 notes) — e.g. a
  runtime `perry_eh_selfcheck()` that performs a real raise across a Rust
  frame, exercised by the test harness.

### Also verified in the spike

- nested try + rethrow-from-catch to the outer pad
- finally-on-exception-path then re-raise
- uncaught → `_URC_END_OF_STACK` → report + exit(1)
- generated frames without personality are stepped through transparently

## Windows

`x86_64-pc-windows-msvc` is a real, CI-exercised target (windows-build job;
doc-tests compile and run TS on windows-2022) and has no
`_Unwind_RaiseException` and no Itanium landing pads. The plan is the SEH
funclet form: `js_throw` → `RaiseException` with a Perry-owned exception code,
`catchswitch`/`catchpad` with personality `__C_specific_handler` and a fixed
filter function matching the code. The invoke-conversion infrastructure in
codegen is shared; only the dispatch/landing shape is per-triple (exactly how
`setjmp_abi` already selects per-triple today). MSVC x64 unwind tables are
mandatory for all functions, so the cross-Rust-frame story has no
force-unwind-tables analogue there.

## Phase 1 — implementation (landed on the branch behind `PERRY_EH=invoke`)

- **Runtime** (`perry-runtime/src/eh.rs` + `exception.rs`): `perry_eh_personality`
  (ported Itanium LSDA walk, catch-all — Rust std's personality trimmed of
  type-table/filter logic, MIT/Apache-2.0), per-thread `PERRYJS\0` exception
  object, `js_eh_try_push()` (same savepoint recording as `js_try_push`, no
  jmp_buf), and a `HandlerKind` per handler-stack entry: `Setjmp` entries
  (old lowering + every Rust-side boundary trap — `js_call_catching`,
  combinators, iterator/timer/promisify traps, all of which pair
  `js_try_push` with `ffi::setjmp`) are reached by `longjmp`; `Unwind`
  entries by `_Unwind_RaiseException`. A raise that returns despite an armed
  handler aborts loudly naming lost unwind tables (the RUSTFLAGS foot-gun).
- **Codegen**: EH scope stack on the shared `RegCounter`;
  `call`/`call_void`/`call_indirect` emit
  `invoke … to label %eh.contN unwind label %lpad` plus a flush-left inline
  continuation label whenever a scope is active and the callee can throw
  (`llvm.*`, `js_shadow_*`/`js_gc_*`, the EH bookkeeping five, and
  `#2/#3/#4`-audited helpers stay plain calls). `lower_try_invoke` mirrors
  the setjmp CFG exactly — same catch-entry sequence, same catch-param
  binding (#7209), same finally duplication — with `emit_eh_dispatch`
  replacing the setjmp dispatch and scope push/pop replacing
  `enter/exit_try_region`. Async rejection boundary converted the same way.
  Invoke-mode functions get `personality ptr @perry_eh_personality` and no
  `#0`/`#1` groups, no volatile pass, no noinline.
- **Return/break/continue inside `try`**: unchanged — finally inlining is a
  HIR-level transform (`perry-transform/src/finally_inline.rs`) whose clones
  sit inside the try body, and its documented limitation (a throwing clone
  routes to the same try's handler) is transport-independent, so behavior is
  bit-identical to the setjmp path.
- **Tooling**: `scripts/gc_root_dominance_check.py` learned `invoke` (CALL_RE
  + CFG edges for both destinations) — otherwise every collecting call inside
  a `try` would be invisible to the dominance analysis, a silent false-green.
  `LlBlock::contains_gc_unsafe_call` (#5093) matches `invoke` too.
  `PERRY_EH` participates in the object-cache key (#6394 rule).
- **Dev profile**: `[profile.eh-abort]` (inherits perry-dev, `panic=abort`)
  + `RUSTFLAGS="-C force-unwind-tables=yes"` builds the runtime archives for
  invoke-mode testing without flipping the workspace default during the flag
  period.

## Acceptance evidence (running)

**Gap suite under `PERRY_EH=invoke`** (2026-08-03, macOS arm64, perry-dev
compiler + eh-abort runtime archives, `PERRY_SKIP_BUILD=1` harness run):
93.9% parity, 20 output mismatches, 9 crashes. Attribution:

- **All 20 output mismatches are mode-independent** — byte-identical output
  and exit codes when recompiled with the same binary/runtime under setjmp
  mode. 8 are in `known_failures.json`; the other 12 are oracle-environment
  (node cannot resolve npm fixtures — `package_json_reader:301`; enum tests
  where `--experimental-strip-types` node errors — `run_main:107`) or
  pre-existing at this main commit under the perry-dev profile (verified for
  `test_gap_6301_event_target_subclass` and `test_gap_4510_enum_forward_ref`
  against the unwind runtime + setjmp transport, i.e. main's exact
  configuration). **Zero invoke-attributable output regressions.**
- The **9 crashes are all in the http/net/fetch family** and are a build-
  coherence artifact of the ad-hoc environment, not a lowering bug: those
  tests link prebuilt `perry-ext-*` archives whose *bundled* runtime was
  compiled `panic=unwind` from older source. A JS unwind crossing an
  ext-archive Rust frame hits the RFC-2945 abort guard — the crash output
  says `panic in a function that cannot unwind` verbatim (probe scenario s4).
  One of the three probed also segfaults identically under setjmp mode
  (`fetch_instanceof_5433`, KNOWN), and `net_connect_bound_value` dies with
  the same tokio no-reactor panic under both modes. Fix: coherent archives
  (rebuilding the http-family ext crates under the eh-abort profile) — which
  the final flip provides globally by putting `panic=abort` on the release
  profile itself.
- The three new #7302 tests pass under invoke mode, **including the GC
  throw-across-collection probe** (200 iterations of allocate-in-try →
  throw across churn → verify caught value, error payload, and the catching
  frame's locals).
- Subject-live check: the traced module IR for the structural-path test
  contains 106 `invoke`/`landingpad`/`personality` lines and zero setjmp /
  `returns_twice` / volatile machinery.

**Smoke corpus** (structural paths incl. the #6385 volatile-hazard shape,
cross-helper throws incl. throwing getter/toString/JSON.parse/map-callback +
500-frame deep unwind, async boundary + generators + Promise.all combinator
interplay): byte-for-byte vs Node 26.5.1 under invoke mode; uncaught-throw
output byte-identical to the setjmp build.

### Performance (macOS arm64, perry-dev compiler, 3-run medians)

Both directions, as promised. Reference: the pinned Node oracle on the same
files (V8's own EH machinery).

| microbenchmark | setjmp | invoke | node/V8 |
|---|---|---|---|
| b1: hot `try` that never throws (200×1M iters) | 5.6 s | **5.0 s** | 0.26 s* |
| b3: small try-containing fn in a hot loop (80M calls) | 2.31 s | **1.95 s** | 0.07 s* |
| b2: throw+catch every iteration (300k shallow throws) | **62 ms** | 451 ms | 110 ms |
| b4: 20k throws × 200-frame unwind | **19 ms** | 4.10 s | 168 ms |

\* b1/b3's node column reflects V8's integer-loop JIT advantage (the known
AOT-vs-V8 integer-math gap), not EH — the EH-relevant comparison there is
perry-vs-perry.

Reading: the non-throwing path — the case zero-cost EH exists for, and the
overwhelmingly common one — gets 10–20% faster (no `_setjmp` per entry, no
volatile-pinned locals, inlining unlocked). The throw path pays the
industry-standard price of real unwinding: ~1.5 µs per shallow throw
(2-phase walk; C++/Swift/Rust are in the same band; V8 pays ~0.37 µs) and
~1 µs per frame stepped on deep unwinds, vs `longjmp`'s O(1) register
restore. The b4 shape (exception-as-control-flow across 200 frames in a hot
loop) is the pathological case at ~24× V8; no gap/parity test moved
measurably. Optimization avenues if a real workload ever hits this:
per-frame step cost is macOS-libunwind-specific (Linux `.eh_frame_hdr`
stepping is cheaper — measure in CI), and generated frames without handlers
carry no personality, so the walk is pure CFI decode.

A register-snapshot "fast transport" (save callee-saveds at try entry, jump
straight to the pad) was considered and rejected: it is `setjmp` by another
name — LLVM's EH model expects each unwound frame's callee-saved registers
restored by the unwinder, so a snapshot restore would resurrect try-entry
values for locals defined after the push, recreating exactly the volatile
problem this migration deletes.

### Final sweep (merged branch, flipped default, coherent perry-dev build)

95.4% parity, 21 output mismatches, **1 crash** (`test_gap_fetch_instanceof_5433`,
KNOWN, crashes identically under the setjmp build). The mismatch set is exactly
the attributed baseline — every entry previously proven mode-independent by
same-binary A/B. The 8 http-family crashes from the flag-period run are gone
with coherently-built archives, confirming the ext-archive profile-mixing
attribution. Zero invoke-attributable regressions, final.

## Follow-up: the owned single-phase unwinder

The migration left one honest regression: throws got slower, because real
unwinding replaced `longjmp`'s O(1) register restore. The system unwinder
walks every frame **twice** (search + cleanup) and re-decodes each frame's
CFI on **every throw** — measured 512 ns per frame-step on macOS.

Perry does not need either property. The handler stack already *is* the
search result (we know the target before raising), and throw paths repeat,
so a decoded frame can be cached. `perry-runtime/src/eh_walker.rs` therefore
carries throws itself: one phase, a per-PC row cache, and a direct register
install. `_Unwind_RaiseException` remains the fallback.

| microbenchmark (20k iters, macOS arm64) | system unwinder | owned walker | node/V8 |
|---|---|---|---|
| deep unwind, 200 frames | 4096 ms | **287 ms** (14.3×) | 168 ms |
| shallow throw+catch | 451 ms | **80 ms** (5.6×) | 110 ms |

Deep-unwind throws went from 24× slower than V8 to within 1.7×; shallow
throws now beat V8. Non-throwing paths are untouched (they were already
10–20% faster than the setjmp era).

### Why this is safe

Every register reload is a raw dereference of a computed address, so a
misdecoded unwind row is not a wrong answer — it is a wild read. Three
layers, each of which had to *prove it ran*:

1. **W0** — the owned walk reproduces `_Unwind_Backtrace`'s frame chain
   exactly (unit differential).
2. **W1** — `PERRY_EH_WALKER=diff` predicts (landing pad, CFA) before every
   raise and asserts it inside the personality against the system unwinder,
   tallying verified/declined at exit so a silent run cannot pass for a
   verified one. Result: 20,000 deep unwinds (~4M frame steps), the GC
   throw-across-collection probe, and the smoke corpus — **zero
   mispredictions, zero declines**. Liveness of the checker itself was
   proven by deliberately corrupting a prediction (+4) and confirming the
   abort fires.
3. **W2** — fail-safe stepping: the CFA must climb a plausible stack
   monotonically and every slot address must lie within the walk's stack
   span, or the walk declines and the system unwinder carries that throw.
   **This was not theoretical**: unguarded, the walker segfaulted stepping
   `libtest`'s frame shapes (which compiled programs never produce).
   Guarded, the differential passes and the program path still reports
   `fallback=0` across 20k+ throws.

`d8..d15` are tracked and restored, not just the integer set — a handler
frame holding a live `f64` across the `try` would otherwise resume with a
stale value, which is silent numeric corruption rather than a crash.

Acceptance: the full gap suite under the owned transport returns **the
byte-identical failure set** as merged main (95.4%, same 21 mismatches, same
single known crash), and the GC probe passes under default,
`PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1`, and
`PERRY_GEN_GC=0`. `PERRY_EH_WALKER=off` reverts to the system unwinder for
bisection (measured: b4 returns to 4231 ms, confirming the knob is live).

### Platform status

aarch64/macOS today. Other architectures and Linux keep the system
unwinder — identical semantics, the old speed — because the walk declines
when it has no image to decode. Linux needs `dl_iterate_phdr` +
`PT_GNU_EH_FRAME` discovery; the stepping and cache above it are
platform-independent. A measured aside for that work: **89% of our
functions are DWARF-mode escapes** in the compact-unwind table, so the
gimli path (not the compact steppers) is the one that matters, and forcing
frame pointers on generated functions would move most of that population
into the trivially-steppable FRAME encoding.

## Phase 1+ design notes (running)

- Handler bookkeeping: `js_try_push` today returns a jmp_buf and the generated
  code setjmps on it. Replacement: `js_eh_try_push()` (void) records the same
  savepoints and a handler kind (`Generated`); `js_call_catching` pushes kind
  `RustCatch` internally with its private jmp_buf. `js_try_end`, catch-side
  `js_get_exception` + `js_clear_exception`, return-inside-try `js_try_end`
  bookkeeping: all unchanged.
- Codegen chokepoints: every call goes through `LlBlock::{call, call_void,
  call_indirect}`; the unwind-label stack lives on the shared `RegCounter`
  (same Rc the try-region store tracking uses today). Inside an active
  handler scope, calls are emitted as `invoke … to label %eh.cont.N unwind
  label %lpad.M` followed by an inline `eh.cont.N:` label line — the LlBlock
  keeps appending, so no caller restructuring. Calls to `#2/#3/#4`-attributed
  helpers (`nounwind willreturn`) and `@llvm.*` intrinsics stay plain calls.
- `has_try` stops meaning noinline+volatile and starts meaning
  `personality ptr @perry_eh_personality` on the define.
- Textual scanners that match `"call "` must learn `invoke` — first found:
  `LlBlock::contains_gc_unsafe_call` (#5093 versioned-loop call-free check);
  a systematic sweep is part of Phase 1.
