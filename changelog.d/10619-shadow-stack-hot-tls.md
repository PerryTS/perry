Give the shadow-stack root state (`SHADOW`) the `tls_hot` fast path. It was the
last savepoint field still declared as a raw `thread_local!`, so every
`CatchSavepoint::capture()` — once per `try` entry — paid `_tlv_get_addr` for
it on Darwin, while `EXCEPTION_STATE`, `CALL_METHOD_DEPTH` and the named
`runtime_handle_stack`/`temp_roots` fields were already routed through
`tls_hot`. Wave 1's "has any thread used this subsystem" latch does not make
the read rare here: it is set by the first shadow-frame push anywhere in the
process, and a `catch (e)` binding needs a shadow slot itself.

Profiling pinned it to one call site, `try_push_with_kind+176 ->
_tlv_get_addr`, about 6% of instructions retired per entry. The change is a
storage-mechanism swap — same type, call sites, const-init and drop-free
semantics — so the address-stability contract `js_shadow_frame_enter` depends
on is unchanged. A non-throwing `try` entry goes 179.3 -> 164.8 instructions
(-8.1%), differenced within each binary with the loop control at the noise
floor in both arms. The probe understates it: `SHADOW` is read on many paths,
not only this one.

Because the shadow stack is the GC's precise root set rather than bookkeeping,
the new fixture attacks rooting: a throw four frames down, a throw crossing
`Array.prototype.map`'s runtime trampoline, `finally` on both paths, nested
`try` with an inner rethrow, a `catch` that itself throws, and 25-level nesting
— every caught value read back after GC-pressure allocation. It plus four
existing exception/rooting fixtures ran under five GC-schedule seeds at
RATE=1 with from-space protection, all byte-identical to node, with
forced_collections=2566 / copying_minors=2566 / moved_objects=37000 confirming
the instrument was live.

**Follow-up: CI's `cargo-test` job (Linux, debug profile) SIGSEGV'd on this
change** (run 35374727647, job 105594641738) — no `FAILED` line survived
(libtest's stdout is block-buffered under CI), so the crashing test was never
named. `cargo-test` was clean on #10644, #10647, #10650, #10651 against the
same base — not a contradiction, once checked: none of those four PRs touch
`shadow_stack.rs`, and this PR was never merged to `main`, so their
`cargo-test` runs (branch merged with `main`) never contained this change to
begin with. Only this branch's own run ever exercised it, and that run was
red.

Reproduction, in the exact failing configuration
(`RUST_TEST_THREADS=1 cargo test -p perry-runtime --lib`, debug, no
`--release`), across every environment tried locally, came back clean and was
ultimately inconclusive:
- macOS arm64, the shipped Darwin `perry_thread_local!` path: 4013 passed.
- macOS arm64 with the Darwin `pthread`-TSD path forced off (`hot()` forced
  onto the generic `hot_via_tls()` route every non-Darwin-aarch64 target
  already uses): 4013 passed, 0 failed.
- Linux x86_64 (qemu-emulated Ubuntu VM on the CI-pinned
  `nightly-2026-08-20` toolchain, matching `ubuntu-latest`'s triple): ran
  clean through 1968+ of ~4013 tests in alphabetical order, well past the
  async_hooks region where CI's log cuts off. qemu turned out to be unusable
  as an instrument beyond that: one specific unrelated regex-replace stress
  test is pathologically slow under emulation (fast natively), and an
  attempted A/B there produced two SIGKILLs that were first misread as a
  reproduced crash — they were an operator `pkill -f` self-matching its own
  remote shell (a documented pitfall), not a fault.

So local reproduction never settled it either way. Causation was confirmed
the direct way instead: pushing this cfg-gated fix and reading CI's own
`cargo-test` job on the exact failing runner — green (run 35433215970, job
105871415920), where the unconditional swap was red. The PR *is* what caused
the SIGSEGV; gating it off Linux/non-Darwin is what fixed it. The internal
mechanism inside `tls_hot.rs`'s resolution path is still not understood —
this is a fix by removing the exposure, not by finding the fault. See #10709
for the open half of the investigation (`fill()`'s `temp_roots`-last
ordering guards against a half-filled cache being *used* re-entrantly, not
against `fill()` being *called* again re-entrantly, which remains a live
suspect). The change is narrowed rather than left as a mystery with no
mitigation:

`SHADOW`'s declaration is now cfg-split —
`crate::perry_thread_local!` only under
`all(target_vendor = "apple", target_arch = "aarch64", target_pointer_width = "64")`,
a plain `thread_local!` (the pre-#10619 form) everywhere else. This isn't
only a hedge against the unconfirmed SIGSEGV: `tls_hot.rs`'s own module docs
already say the published-cache shortcut is Darwin-aarch64-specific, and
everywhere else "resolving a thread-local is already a fixed offset and the
extra cache indirection has no demonstrated benefit" — so routing `SHADOW`
through it unconditionally was strictly more work (one extra `HOT`
resolution plus a slot-array indirection) on every other target for a win
that was only ever measured on Darwin. `scripts/thread_local_cold_allowlist.json`'s
`shadow_stack.rs` count goes back to 2 (its pre-#10619 value) to match.

Re-verified on this Darwin host post-split, differential probe (own builds,
base = this PR's parent 9df5075fbe, arm = this fix, both
`PERRY_NO_AUTO_OPTIMIZE=1`, median of 7, N=20000/40000): try-entry marginal
cost 162.2 -> 152.0 instructions/entry (-10.2, ~6.3%), bare-loop control
~1.0 instructions in both arms (noise floor relative to the ~150-instruction
signal) — the win the original commit measured (-8.1%) survives, because the
fix does not touch the Darwin code path at all.

Also re-ran: debug and release `cargo test -p perry-runtime --lib` (release:
4010 passed, the same 2 pre-existing `debug_assert!`-gated failures noted
above); the gap fixture plus GC-stress (seeds 1 and 42,
`PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_PROTECT_FROMSPACE=1
PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_FROMSPACE_SCAN_ABORT=1`) both
byte-identical to node with non-zero copying minors
(`copying_minors=76 moved_objects=35258`) and a live
`[gc-fromspace-protect] retired_set=#75` line; `cargo fmt --all -- --check`;
`scripts/run_lint_gates.sh` (the only failure is the pre-existing, known-red
public-baseline step); `scripts/check_thread_locals.py`/`--self-test`.
