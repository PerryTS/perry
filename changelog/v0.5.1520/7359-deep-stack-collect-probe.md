### Tests

**GC ratchet: a probe that collects at stack depth, and walker-liveness gating on every platform.**

The native-root stack walker had no probe that exercised it. Every probe in
`benchmarks/gc_ratchet/probes/` calls `gc()` at the end, from a shallow stack,
so on macOS and Linux even `04_dead_after_deep_stack` reported
`frames_visited: 7, locations_visited: 0` — **zero** root locations. Both arms
would have passed the whole suite unchanged with a walker that visited nothing,
because other root sources covered those probes. Windows walked a deep stack
(5,626 frames) only by accident of heap sizing, and that accident is the sole
reason the `--require-locations` gate added in #7354 could be applied there and
nowhere else. This is the fourth failure mode in CLAUDE.md's list: the gate ran,
but its subject never did.

`11_collect_at_depth` makes the coverage deliberate. `descend` holds a heap
value live *across* its recursive call and collects at the deepest point, so at
collection time there is one live root per frame, all mid-frame rather than in
the leaf. Each slot is read after the collection returns, so a walker that stops
early — or a map with a wrong base register — yields a wrong checksum rather
than a merely slower run; under `PERRY_GC_FORCE_EVACUATE=1` every survivor
moves, so a stale pointer cannot be accidentally correct.

Measured against the pinned Node oracle, byte-identical on both:

| arm | frames | locations | before |
|---|---|---|---|
| macOS aarch64 | 228 | 221 | 7 / 0 |
| x86-64 Linux | 231 | 221 | 7 / 0 |

With both Unix arms now walking a real stack, `gc_walker_trace_assert.py
--require-locations` moves off the Windows-only branch in
`.github/workflows/gc-native-roots.yml` and gates all three arms. Full ratchet
suite: 11/11 byte-identical under `PERRY_RS4GC=1 PERRY_GC_FORCE_EVACUATE=1
PERRY_GC_VERIFY_EVACUATION=1`.
