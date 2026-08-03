# Exception lowering: setjmp/longjmp → LLVM `invoke`/`landingpad`

Status: **in progress** — Phase 0 (spike) complete, Phase 1 (codegen) underway.
Branch: `exp/invoke-eh`. Development flag: `PERRY_EH=invoke|setjmp` (temporary —
deleted when the default flips; a permanent hybrid is the failure mode this
work exists to remove).

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
