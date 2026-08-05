**fix(runtime): the full `perry-runtime --lib` suite is green on Windows — SEH-safe longjmp, setjmp alignment, and four platform-shape test fixes (#7356)**

#7355 made perry-runtime *compile* on Windows; running the suite then surfaced the pre-existing failures inventoried in #7356. This lands the fixes and the CI arm that keeps them fixed. `cargo test -p perry-runtime --lib -- --test-threads=1` on Windows 11: **1635 passed, 0 failed** (previously: three process-killing stoppers truncated every run).

Two of the fixes are production bugs, not test bugs:

- **`js_throw`'s `longjmp` was undefined behavior on windows-msvc** (`exception.rs`). MSVC's `longjmp` reads `_JUMP_BUFFER.Frame` (the jmp_buf's first 8 bytes) and, when nonzero, performs a REAL `RtlUnwindEx` stack unwind — and our one-arg `setjmp` extern leaves that slot holding garbage (the CRT `_setjmp` stores its *second* parameter there; we pass one). Measured: STATUS_BAD_STACK (0xC0000028) in a release probe, GS-cookie aborts (`_report_gsfailure`) under the panic=unwind test harness — the `dyn_eval`/`native_abi` stoppers. Every Rust-side boundary-trap catch on Windows (microtask pump, `js_call_catching`, iterator trampolines, promise combinators) rode this path. Fix: zero the Frame slot before the jump, forcing the non-unwinding POSIX-style `longjmp` whose skipped-cleanup semantics the savepoint restores in `js_throw` already assume. End-to-end validated with a compiled probe (throwing `.then`, `Array.from` mapper, `Promise.all` member, plus a 1000-iteration churn loop) byte-identical to the Node oracle.
- **The conservative-scan register snapshot buffer was under-aligned** (`gc/roots.rs`). MSVC's `_setjmp` saves XMM registers with aligned stores; the `[u64; 32]` buffer is 8-aligned, an immediate access violation whenever it lands 8-mod-16 (measured; this was the `ffi::setjmp` smoke-test AV, same root cause). The snapshot buffer and the three test buffers are now `repr(align(16))`; the extern's docs record both MSVC contracts.

Test-shape fixes, each keeping the subject live on Windows rather than skipping:

- `date`: the TZ-isolation child now uses `TZ=PST8PDT` on Windows — the UCRT's `TZ` parser silently degrades IANA ids to UTC, which failed the test's own subject-is-live guard.
- `gc` malloc-trim: the test counter now records that budgeted reclaim *reached* the trim call (the #6180 subject) rather than only counting the glibc/macOS executing arms, which made the gate unsatisfiable on platforms with no trim primitive.
- `child_process`: `spawnSync` result-shape test spawns `cmd /c echo hi` on Windows (`echo` is a shell builtin there, ENOENT under Node too).
- `ffi::setjmp` smoke tests: aligned buffers (above).

CI: the `windows-build` job now runs `RUST_TEST_THREADS=1 cargo test --profile perry-dev --lib -p perry-runtime` — same single-threaded invocation as the ubuntu leg (#1444), perry-dev profile so it shares the job's build artifacts. Before #7355 there was zero Windows CI to notice the crate didn't compile; this step is what keeps the suite from rotting back to "unmeasurable".

Out of scope, recorded for honesty: the suite has 5 pre-existing failures in *parallel* mode (`closure::dynamic_props`, `gc teardown`, `prop_plan`, `global_this_webassembly`) — cross-thread interference that CI already sidesteps on every platform by running single-threaded.
