### GC: a Web Fetch handle id is not a heap address (SIGSEGV on `instanceof`)

`fetch_subclass_handle_id` classified its receiver by **magnitude** rather than
by **band**: a floor of `GC_HEADER_SIZE + 0x1000` plus `is_valid_obj_ptr`,
whose own `HEAP_MIN` is `0x1000`. Every Web Fetch handle id clears that — they
live in `[0x40000, 0xE0000)`, far above the floor and far below
`HANDLE_BAND_MAX` (`0x100000`) — so the probe dereferenced a small integer at
`addr - 8`.

`new Response(...)` yields id `0x40000` exactly, and `r instanceof Request`
reaches this probe, so **the first fetch handle a program allocates
segfaulted**: `test_gap_fetch_instanceof_5433` died at address `0x3fff8` after
printing two lines, deterministically, on two hosts.

The guard is now `addr_class::is_plausible_heap_addr` — the canonical
`is_above_handle_band && is_valid_obj_ptr` pairing — and the header read goes
through `addr_class::try_read_gc_header`, which magnitude-classifies before
touching memory. The regression test walks the band *boundaries* rather than a
single value, so a band added to `addr_class` without a matching guard here
fails it; restoring the old guard and raw deref SIGSEGVs the test binary.

### Gates: the gap snapshot may no longer park a crash

The crash was invisible because it was **accepted** in `gap_snapshot.json` as
`status: "crash"`, citing #5433 and #5917 — **both closed** — while marked
`category: "bug-open"`. The harness already printed crashes every run, but
printing is not gating, so a segfault sat in the expected-*output* channel for
a month.

`run_gap_tests.sh` now refuses any `status: "crash"` snapshot entry outright,
enforcing the policy its own comment four lines up already stated. It starts
green (this change removes the only such entry) and can only go red on a new
attempt to park a crash; planting a probe entry makes it exit 2.
