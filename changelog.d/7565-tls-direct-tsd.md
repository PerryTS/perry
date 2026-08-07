### Allocation path: reaching the hot-TLS cache stops being a call (#7469)

#7474 cached the *addresses* of the thread-locals on the allocation path in one
`const`-initialised thread-local, collapsing a dozen `_tlv_get_addr` calls per
object literal down to one. That one remained: `HOT` is itself a
`thread_local!`, so every runtime function reading any hot field still paid a
call into `libdyld`. It is now **1.1% of `churn_alloc` self time, from 27.0%**,
and the three allocation probes are 1.14x–1.18x faster.

**Re-measured before scoping**, because three tickets in this campaign were
worked off stale headline numbers. Symbolicated `sample` on the pinned quiet
host at `9938cbc1a`, 923 leaf samples: `_tlv_get_addr` is 27.0% — the headline
had not drifted, but the *shape* had. The ticket opened saying "41 distinct
call-graph sites — this is diffuse, not one hot caller". It is not diffuse any
more. **Seven functions carry 98% of it, and every one of the seven resolves
`HOT`:**

| caller | share of `_tlv_get_addr` | of total |
|---|--:|--:|
| `gc::barrier::write_barrier_decoded_parent` | 19.3% | 5.2% |
| `gc::layout_tables::layout_forget_object` | 18.9% | 5.1% |
| `js_object_alloc_class_inline_keys` | 18.5% | 5.0% |
| `arena::allocators::arena_alloc` | 14.9% | 4.0% |
| `js_write_barrier_slot` | 9.2% | 2.5% |
| `gc::barrier::barrier_child_prologue` | 8.8% | 2.4% |
| `gc::layout::typed_shape_layout_entry` | 8.4% | 2.3% |

Two of the seven resolve *nothing else*. (Naming which thread-local each site
resolves needs a static census rather than a grep: on Mach-O there is no
`bl _tlv_get_addr` in the text at all — the call is indirect through the TLV
descriptor, so the census walks `adrp`/`add` pairs landing in `__thread_vars`.)

That attribution chose the design. The lever is the accessor, not the call
graph: the ticket's "thread a context pointer through generated code" would
have to cross every runtime FFI boundary against 2994 `.with()` sites over 255
`thread_local!` blocks, while making `hot()` free fixes all seven at once.

On Apple aarch64 the pthread thread-specific-data array is directly addressable
from `TPIDRRO_EL0` — that is how `pthread_getspecific` itself is implemented,
and what mimalloc (already linked into this runtime) does on this platform.
`tls_hot` publishes the cache's address into one `pthread_key_create` slot and
reads it back inline: `mrs` plus two loads, no call, no caller-saved-register
clobber. Every other target keeps the previous path unchanged.

**The asm must not be `pure`, and that was learned the hard way — recorded
here because the reasoning that produced the bug was persuasive.** Marking it
`options(pure, nomem)` is the obvious move: masked of its CPU-number bits the
register is a per-thread constant, so `pure` lets LLVM CSE the read across a
whole function. It is also wrong. `pure` promises the result depends only on
the inputs, and this asm has none, so LLVM may compute it once and reuse the
value anywhere in the function — *including across a point where execution
resumes on a different thread*. `perry-stdlib`'s async bridge is exactly that
shape: `hot()` is `#[inline(always)]` and LTO inlines it into futures tokio
polls, so a hoisted thread pointer outlives the thread it was read on. Every
`node:net` / `node:http` server aborted with tokio's "there is no reactor
running" — 5/5 against 5/5 clean on `main`.

The tempting counter-argument, that `@llvm.threadlocal.address` is already
`speculatable` + `memory(none)` so a `thread_local!` read had the same freedom,
does **not** hold: on Darwin that intrinsic lowers to a *call* through the TLV
descriptor, which LLVM will not hoist across arbitrary code. Replacing the call
with inline asm is what made the hoist possible. Two further notes for whoever
meets this next: nothing cheaper than the gap suite found it — 1811 runtime
unit tests, 12 GC-ratchet probes, and the whole allocation benchmark set are
all green with the bug present — and dropping `pure` costs **nothing
measurable**, with all three probes landing on the same millisecond as the
`pure` build.

**It cannot silently read a wrong address.** The publishing thread reads its
slot back *through the direct path* and compares it against what
`pthread_setspecific` was handed; a mismatch — the shape a future OS change
would take — latches the direct path off process-wide and every thread reverts
to `_tlv_get_addr`. A fresh thread reads null and takes the cold path, which is
POSIX ("upon thread creation, the value NULL shall be associated with all
defined keys in the new thread"), not an implementation detail.

**And the fast path is asserted live, not assumed.** Every other test in
`tls_hot` passes identically whether `hot()` costs a call or three
instructions, so a silent fallback would make the change inert with nothing
red. `direct_tsd_path_is_live` fails if the direct path was disabled;
`direct_read_matches_pthread_getspecific` checks the open-coded read against the
real libpthread implementation for the same key rather than against our belief
about it; `a_fresh_thread_publishes_its_own_slot` covers worker threads.
Statically, the nine `HOT` descriptor materialisations across those seven
functions are gone from the emitted binary, replaced by `TPIDRRO_EL0` reads
(whole-binary `mrs` count 104 → 482).

A second commit takes the residue. With `HOT` free, `_tlv_get_addr` fell to
3.5% and **100% of what was left attributed to one caller** —
`js_object_alloc_class_inline_keys`, i.e. `learned_inline_field_count`, which
runs on every dynamic construct. `LEARNED_INLINE_FIELDS` joins the cache under
the same four-step contract. The other thread-locals those functions name
statically were deliberately left alone because the profile does not reach
them: `MARK_SEEDS` and `WRITE_BARRIER_TRACE_COUNTERS` sit behind cold gates,
and `ARENA_TOTAL_BYTES` / `OLD_GEN_IN_USE_BYTES` in `arena_alloc` only move
when a block is installed.

Pinned quiet host, arms interleaved round by round so load drift hits both
equally, best-of-7 after a discarded warm-up:

| probe | `main` | +direct TSD | +learned-inline | total |
|---|--:|--:|--:|--:|
| `churn_alloc` — object literal + push | 1.294 s | 1.134 s | **1.109 s** | **1.167x** |
| `churn` — literal + push + read back | 1.649 s | 1.411 s | **1.403 s** | **1.175x** |
| `push_cls` — `new Node(v,w)` + push | 1.263 s | 1.117 s | **1.104 s** | **1.144x** |

Peak RSS flat (25.2 → 25.2–25.4 MB); program output byte-identical across all
three arms on all three probes. `cargo test -p perry-runtime`: 1811 passed, 0
failed. All three probes under
`PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`
exit 0 with 110 `[gc-fromspace-protect] mode=ProtectPages retired_set=#N` lines
each — the count is quoted rather than the exit code, because a run with zero
copying minors protects nothing.

The GC ratchet was measured against a **same-session `main` build** rather than
argued from the filed numbers: both arms out of one target directory with the
identical `-p` set, back to back with the same harness. Both report the same ten
gating breaches with the same values, #7559's `05_closure_capture` +16.44% and
`02_survivor_promotion` +2.77% among them — this change moves none of them in
either direction. Cell by cell that is **156 cells over 12 probes with exactly
one semantic difference**, `12_large_live_set.heap_used_bytes` at +0.004%
(2,304 B), which is the single cell the ratchet's own `probe_override` excludes
as conservative-stack-scan noise (#7554/#7558, spread 9,072 B over 36 runs).
Every other retention and evacuation counter is bit-identical and every probe
checksum matches; `wall_ms` is faster on all twelve. The diff tool asserts it
compared a non-zero number of cells before printing a verdict, because the
metrics are `{samples, median, …}` dicts and a naive numeric read compares zero
of them and reports a vacuous "identical".

The gap suite (466/491 on this host) was what caught the `pure` bug, and it is
also how the rest of the divergence set was cleared: after the fix, five of the
six network aborts pass, and the sixth
(`test_gap_http_client_no_redirect_follow`) fails **byte-identically on
`main`**. The seventh crash,
`test_gap_gc_same_module_call_argument_rooting`, is a harness timeout rather
than a defect: standalone under its own `parity-env`
(`PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_ZEAL=1`) it exits 0 in 13.5 s with
output byte-identical to node, against the harness's 10 s cap. Worth recording
separately: **the gap gate cannot render a verdict on macOS at all** —
`run_gap_tests.sh` selects `test-parity/gap_snapshot.${platform}.json` for
non-Linux hosts and `gap_snapshot.macos.json` is not in the repo, so the run
ends in `FileNotFoundError` and its exit 1 is a missing baseline, not a
regression list.

**This closes the lever, not the ticket.** What is left of `_tlv_get_addr` is
`RuntimeHandleScope`, not the allocation path, so further thread-local work
here is worth at most ~1% — the ceiling in both directions. #7469's remaining
workstreams (codegen emitting the bump allocation inline; per-object footprint)
are untouched.
