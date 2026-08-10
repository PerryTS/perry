// GC ratchet probe: the grow-THEN-churn transition (#7737 item 3).
//
// gc-ratchet-env: PERRY_GC_SCAVENGE_NURSERY_MB=1
// gc-ratchet-env: PERRY_GC_MAJOR_PACING_FLOOR_MB=1
//
// Every other probe in this suite sits at one END of the major-pacing backoff
// (#7726) and never crosses between them:
//
//   * `retain.ts`-shaped growth is pure — every full it escalates reclaims
//     almost nothing, so the backoff shift climbs and STAYS at its cap;
//   * `tree.ts`-shaped churn is always high-yield — every full reclaims most of
//     the heap, so the shift is pinned at 0 and pacing never moves.
//
// The untested middle is a workload that is the first and then becomes the
// second. `arena_growth_full_escalation_due` escalates a minor to a full once
// arena in-use clears `max(floor, baseline << (1 + shift))`, and
// `update_major_pacing_backoff` raises the shift whenever a full reclaimed less
// than `MAJOR_PACING_PRODUCTIVE_YIELD_PCT` (20%). So a warm/cache-build phase
// drives the shift to its cap of 2 on growth alone — correctly, there was
// nothing to reclaim — and the workload then inherits 8x pacing for a phase
// whose garbage profile is completely different.
//
// That is a footprint question rather than merely a pacing one because
// array-growth forwarding stubs (`js_array_grow` leaves one per reallocation)
// are reclaimable ONLY by a full mark-sweep: a non-moving minor sweep cannot
// free them and they pin the arena blocks they sit in, while
// `old_reclaim_pressure_due` tracks old-gen occupancy rather than stub bytes,
// so it does not substitute for that path. The churn phase below therefore
// accumulates pinned blocks that nothing but the escalated full can return,
// while the escalation it needs has been pushed out by a phase that has already
// ended.
//
// WHAT THIS MEASURED, which is not what #7737 predicted. The issue reasoned
// that the delayed reclaim would cost "up to 4x more transient RSS" on this
// shape. A same-binary A/B — this exact probe at its shipped constants, one
// runtime built with `MAJOR_PACING_BACKOFF_SHIFT_MAX = 2` (shipped) and one
// with `0` (backoff disabled), archives rebuilt and mtime-verified between
// arms, stdout byte-identical across both — says otherwise:
//
//   metric                        cap 2 (shipped)   cap 0 (no backoff)
//   escalation boundary reached      56.3 MB            14.1 MB
//   full mark-sweeps                       3                  5
//   copying minors                        24                 24
//   objects moved                    306,715            306,715
//   peak RSS, 3 runs                70.5 70.2 70.5     72.6 72.6 72.6
//
// The boundary does move 4x, exactly as the issue reasoned. Peak RSS does not
// follow it — and does not even move in the predicted DIRECTION: the shipped
// backoff is 2.2 MB (3%) LOWER, reproducibly, while running two fewer full
// mark-sweeps for identical collector work (same 24 copying minors, same
// 306,715 objects moved). Footprint on this shape is set by the steady state
// the minors hold, not by cumulative stub debt, so the escalation boundary is
// never the binding constraint and moving it 4x buys the fulls back for free.
//
// Provenance, because an unlabelled number is how this repo has been wrong
// before: M1 Max laptop, `perry-dev` profile, `PERRY_GC_SCAVENGE_NURSERY_MB=1
// PERRY_GC_MAJOR_PACING_FLOOR_MB=1`, peak RSS from plain runs under
// `/usr/bin/time -l` with no GC tracing enabled (tracing perturbs RSS, which is
// why the counters and the footprint are read from separate runs). This is not
// the pinned M1 mini that owns the baseline artifact: read the table as the A/B
// it is, and the pinned cells for this probe as whatever
// `baseline/gc-ratchet-v1.json` records.
//
// That is why this probe pins rather than asserts a bound: the number to
// defend is the measured one, not the predicted one. `check` fails a probe
// whose run reached no collection, so a future change that stops reaching this
// state fails rather than quietly measuring something else — and one that makes
// the delayed reclaim start to bind moves `rss_bytes` off the pin and reports
// it.
//
// Why the two knobs are declared rather than measured at the shipped defaults:
// the mechanism is a RATIO (the shift climbs per unproductive full; the
// threshold is baseline x 2^(1+shift)), so it reproduces at any scale, but its
// ABSOLUTE scale is set by two things this probe does not otherwise control —
// the nursery cap, which decides how early the first collection lands and
// therefore how small the first post-full baseline is, and the pacing floor. At
// the shipped 16 MB / 32 MB the same three escalations need a live set in the
// hundreds of MB and a churn excursion approaching a gigabyte: faithful, and
// unaffordable seven times per measurement on shared CI. At 1 MB / 1 MB the
// shift still walks 0 -> 1 -> 2 and stops at its cap, against a 14 MB baseline.
// Measured, not assumed: at these knobs `PERRY_GC_TRACE=1` reports the shift
// reaching 2 and the boundary landing at 56.3 MB, and `PERRY_GC_DIAG=1` reports
// 24 copying minors moving 306,715 objects — so the small nursery has not moved
// the workload off the copying minor, which is the failure mode this suite has
// paid for most often.
//
// Metric contract as everywhere else: stdout carries only `probe:`/`checksum:`
// style lines, `#gcmetric` goes to stderr.

declare function gc(): void;

// Phase A: an all-live cache, no garbage. Sized so the collection cadence set
// by the 1 MB nursery lands three escalated fulls against a live set that only
// grows, which is what walks the shift 0 -> 1 -> 2 (its cap).
const CACHE = 300000;
// Phase B: array-growth churn. LENGTH is past several reallocation boundaries
// so each array abandons element storage more than once, and each array is
// dropped almost immediately: its bytes are unreachable, but the stub it leaves
// pins its block until a full runs.
const CHURN_ARRAYS = 40000;
const LENGTH = 384;
const RING = 32;

class Row {
  id: number;
  weight: number;
  peer: Row | null;
  constructor(id: number) {
    this.id = id;
    this.weight = (id * 3) | 0;
    this.peer = null;
  }
}

const cache: Row[] = [];
for (let i = 0; i < CACHE; i++) {
  cache.push(new Row(i));
}
// An intra-cache edge, so the retained set is a graph rather than a flat array
// the collector can walk in allocation order.
for (let i = 0; i < CACHE; i++) {
  cache[i].peer = cache[(i * 7 + 3) % CACHE];
}

// Read the cache back, so the growth phase is genuinely live at the point the
// escalated fulls run rather than storage the compiler could sink.
let cacheSum = 0;
for (let i = 0; i < CACHE; i++) {
  cacheSum = (cacheSum + cache[i].id) | 0;
}

// The ring is load-bearing for the same reason it is in 01_nursery_churn: an
// array that never escapes gets scalar-replaced and the probe measures a
// collector that never ran.
const ring: (number[] | null)[] = [];
for (let i = 0; i < RING; i++) {
  ring.push(null);
}

let churnSum = 0;
for (let n = 0; n < CHURN_ARRAYS; n++) {
  const a: number[] = [];
  for (let i = 0; i < LENGTH; i++) {
    a.push((n + i) | 0);
  }
  ring[n & (RING - 1)] = a;
  churnSum = (churnSum + a[LENGTH - 1]) | 0;
}
for (let i = 0; i < RING; i++) {
  ring[i] = null;
}

// The cache is still live across the whole churn phase — that is what holds the
// pacing baseline up and the escalation far away. Reading it back through the
// intra-cache edge also proves the growth phase's objects survived every
// collection the churn phase triggered.
let liveSum = 0;
for (let i = 0; i < CACHE; i++) {
  const r = cache[i];
  liveSum = (liveSum + r.weight) | 0;
  const p = r.peer;
  if (p !== null) {
    liveSum = (liveSum + (p.id & 1023)) | 0;
  }
}

gc();
const mu = process.memoryUsage();

console.log("probe:14_grow_then_churn");
console.log("cacheSum:" + cacheSum);
console.log("churnSum:" + churnSum);
console.log("liveSum:" + liveSum);
console.error("#gcmetric heap_used_bytes=" + mu.heapUsed);
console.error("#gcmetric heap_total_bytes=" + mu.heapTotal);
console.error("#gcmetric rss_bytes=" + mu.rss);
