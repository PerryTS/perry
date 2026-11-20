### Allocation path: one `_tlv_get_addr` per runtime call instead of a dozen (#7469)

With the collector no longer the limit on allocation-heavy programs (total GC
pause on `gc-handoff/bench/churn.ts` is 0.03 s of a 4.3 s run), the cost moved
into the mutator's allocation path — and 27.9% of self time there was
`_tlv_get_addr`, macOS's thread-local accessor.

On Darwin every `thread_local!` access is an out-of-line call into `libdyld`.
Unlike ELF's `local-exec` / `initial-exec` models it is neither inlined nor
cached across accesses, and — the part that makes it add up — LLVM can CSE
repeated accesses to the *same* thread-local, but two different thread-locals
are two different descriptors. So N distinct thread-locals on one code path cost
N calls however well that path inlines. A single `{v, w}` object literal touched
about a dozen: the arena and its inline bump state, the free-list flag, the
allocate-black birth flags, three layout side tables, the page-generation cache
the write barrier classifies against, and the temp-root stack.

`crates/perry-runtime/src/tls_hot.rs` caches those *addresses* in one
`const`-initialised thread-local. The storage does not move: every slot holds
the address of the existing `thread_local!` in its owning module, so
initialisation order, lazy init and destructor registration are unchanged. The
slots are untyped (`*mut u8`) so each owning module keeps its storage type
private, which means a mis-wired `fill()` would hand out a *well-typed*
reference to the wrong object —
`tls_hot::tests::cached_addresses_match_thread_locals` asserts every
address/accessor pairing, and `every_slot_is_populated` asserts nothing was
skipped.

Profiling the remainder (symbolicated `sample`, attributing each
`_tlv_get_addr` to its immediate caller) turned up four more things:

- **`incremental_mark_barrier_value` read a thread-local on every heap-pointer
  store** — 91 of 653 attributed samples, every one spent proving a null pointer
  was still null. It now consults the process-global
  `PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT` first, the same authority
  generated code already trusts for this exact question. Consequently
  `incremental_mark_barrier_enable` now arms that count **before** installing
  the thread-local pointer (`disable` already cleared the pointer before
  decrementing), so no window exists where the pointer is live while the count
  reads idle. That ordering used to be merely tidy; a store landing in such a
  window would now skip its insertion barrier, i.e. lose a mark, so the
  requirement is written down at the site.
- **`js_array_length` probed the `Map` and `Set` registries on every call** — an
  `arr.length` loop condition paying two thread-local hash lookups. Monotone
  "has anything ever been registered" flags answer for programs that use
  neither. Monotone on purpose: a maintained count is a count that can be got
  wrong across eight mutation sites, and this only ever needs to prove that
  *nothing exists*.
- **The page-generation cache held one entry**, while the write barrier
  classifies at least two unrelated addresses per store (the child written, then
  the parent written into). On `churn.ts` those sit in different 1 MiB
  generation classes, so consecutive classifications evicted each other and the
  cache missed on essentially every call — 71 self samples in the authoritative
  map lookup it exists to avoid. Now 4-way, and behind an `UnsafeCell` rather
  than a `Cell`: `Cell::get` returns a *copy*, and copying the set per
  classification cost more than the lookup (caught as a 2% `retain.ts`
  regression while bisecting the arms of this change).
- **`layout_forget_object`** takes one `borrow_mut` per map instead of a
  `borrow` to test emptiness followed by a second `borrow_mut` to remove.

`gc/hot_tls.rs` holds the `gc`-side accessor/provider pairs, split out because
`barrier.rs` crossed the 2000-line cap; keeping each pair together is what makes
the untyped casts reviewable.

Measured on `gc-handoff/bench`, arms interleaved round by round so machine-load
drift hits both equally, best-of-5 user CPU on a quiet host: churn **1.16x**,
cycles **1.10x**, retain1 **1.08x**, retain **1.07x**, deeplist **1.07x**, tree
**1.03x**. Peak RSS flat (worst cell +0.9%), program output byte-identical on
all six. `_tlv_get_addr` falls to 23.5% of leaf samples; attributed callers 653
→ 542.

The collector is untouched, and that is checked rather than assumed. The pinned
`gc_ratchet` artifact currently fails on `main` itself (28 regressions predating
#7432/#7443/#7449), so clean `main` was measured with the same harness on the
same host and diffed: all 108 compared metrics agree except `heap_used_bytes` on
2 of 12 probes at ≤0.8%, which moves on *different* probes per build and is
therefore allocation-boundary jitter. Every metric in the `gc` family
(`minor_cycles`, `step_cycles`, `copied_objects`, `copied_bytes`,
`promoted_objects`, `promoted_bytes`, `freed_bytes`) is identical across all 12
probes. Per-cycle `PERRY_GC_TRACE` is unchanged on churn (105 cycles, 0.03 s
total pause), tree (43) and retain (11), with tree copying volume holding at the
post-#7432 level of 0.017 GB / 0.2 M object-copies.

This does not close #7469. The floor of this design is *one* resolution per
runtime FFI call, and generated code makes roughly a dozen per object literal —
`js_gc_temp_root_push` / `_get` / `_truncate` alone are 206 of the 542 remaining
samples, three separate calls around one allocating expression. Getting under
the ticket's 5% target needs the call *count* to fall, which is workstream A
item 4 (codegen emitting the bump allocation inline). Workstream B (per-object
footprint) is untouched here.
