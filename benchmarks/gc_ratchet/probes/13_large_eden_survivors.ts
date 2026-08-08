// GC ratchet probe: a survivor cohort carried across LARGE, INFREQUENT
// copying minors.
//
// gc-ratchet-env: PERRY_GC_SCAVENGE_NURSERY_MB=64
//
// Every other probe in this suite runs at the shipped 16 MB nursery cap, so
// every copying minor they exercise is small and frequent. #7472/#7481 both
// faulted at a *non-default* cap and neither was reachable from this matrix:
// #7481 is deterministic at `PERRY_GC_SCAVENGE_NURSERY_MB=64`, absent at 1, 4,
// 16 and 32, and its own issue names the missing coverage — "a live
// copying-minor correctness signal at exactly the cadence the ratchet probes
// never exercise". This probe is that cadence, pinned.
//
// Why the cadence is a different collector, not merely a slower one:
//
//   * a minor at 64 MB has ~4x the Eden to scan and copies a survivor cohort
//     that has had ~4x as long to accumulate, so the copy loop, the survivor
//     semispace sizing and the promotion decision all run at magnitudes the
//     16 MB probes never reach;
//   * `gc/tenuring.rs` sizes the target survivor occupancy as a fraction of
//     the *effective* cap and scales that cap from measured influx, so a
//     larger base lands on a different point of both policies;
//   * a reference that survives N collections at 16 MB survives ~N/4 at 64 MB,
//     which is what moves a missed reference rewrite from "faults on the next
//     cycle" to "faults much later, somewhere else".
//
// What the workload holds across those minors, in the shape the two faults
// took — a slot in a long-lived object pointing at a young object that the
// copying minor relocates:
//
//   * `keep` is a survivor cohort that outlives every collection;
//   * `keep[j].note` takes a pointer to a *freshly allocated* object every
//     256th iteration, so the remembered set and the old-to-young rewrite are
//     exercised at every minor rather than only at the start;
//   * `keep[j].peer` is an intra-cohort edge, so the cohort is a graph and not
//     a flat array the collector can walk in allocation order;
//   * `tag` is a heap string, so a survivor carries a pointer the rewrite must
//     follow into a different object kind;
//   * the verification phase reads all four back and folds them into the
//     checksum, which is diffed byte-for-byte against Node. A missed rewrite
//     therefore has to either fault or produce a wrong number; it cannot pass.
//
// Metric contract as everywhere else: stdout carries only `probe:`/`checksum:`
// style lines, `#gcmetric` goes to stderr.

declare function gc(): void;

// KEEP is sized for the *pacing*, not for the workload. `gc/policy.rs`'s
// `arena_growth_full_escalation_due` escalates a minor to a full mark-sweep
// once arena in-use clears 32 MB AND exceeds twice the post-full baseline, so a
// large Eden on top of a *small* retained set escalates every collection and
// runs zero copying minors — the arm would be pinned on a collector it never
// reached. A ~30 MB retained set holds the baseline high enough that a 64 MB
// Eden is not a doubling, which is what leaves the copying minor reachable.
// Measured on the pinned host at v0.5.1376: KEEP 131,072 -> 1 copying minor,
// KEEP 262,144 -> 3 (freed 36 MB, 36 MB, 68 MB per minor, against 16 MB per
// minor for the default-cap probes). `check`'s liveness rule is what keeps that
// honest: a shape that stopped reaching the copying minor cannot be pinned.
const KEEP = 262144;
const RING = 512;
const CHURN = 3000000;
const NOTE_EVERY = 256;
const NOTE_SHIFT = 8;

class Row {
  id: number;
  tag: string;
  hits: number;
  peer: Row | null;
  note: { a: number; b: number } | null;
  constructor(id: number) {
    this.id = id;
    this.tag = "row-" + (id & 255);
    this.hits = 0;
    this.peer = null;
    this.note = null;
  }
}

const keep: Row[] = [];
for (let i = 0; i < KEEP; i++) {
  keep.push(new Row(i));
}
for (let i = 0; i < KEEP; i++) {
  keep[i].peer = keep[(i * 7 + 3) & (KEEP - 1)];
}

// The ring is load-bearing for the same reason it is in 01_nursery_churn: an
// object that never escapes gets scalar-replaced and the probe measures a
// collector that never ran.
const ring: ({ a: number; b: number } | null)[] = [];
for (let i = 0; i < RING; i++) {
  ring.push(null);
}

let checksum = 0;
for (let i = 0; i < CHURN; i++) {
  const t = { a: i, b: (i * 3) & 4095 };
  ring[i & (RING - 1)] = t;
  const k = keep[i & (KEEP - 1)];
  k.hits = (k.hits + 1) | 0;
  if ((i & (NOTE_EVERY - 1)) === 0) {
    // Old-to-young pointer store into a cohort member that has already
    // survived collections: the remembered-set entry and the reference
    // rewrite this probe exists to exercise. The target walks the cohort
    // rather than reusing the same few slots, so the notes still reachable at
    // the end are spread across it instead of being 32 repeatedly-overwritten
    // entries.
    keep[(i >> NOTE_SHIFT) & (KEEP - 1)].note = t;
  }
  checksum = (checksum + t.b) | 0;
}

for (let i = 0; i < RING; i++) {
  ring[i] = null;
}

// Verification phase. Every field read here has crossed every copying minor
// the run performed.
let hits = 0;
let tagChars = 0;
let peerIds = 0;
let notes = 0;
let noteSum = 0;
for (let i = 0; i < KEEP; i++) {
  const k = keep[i];
  hits = (hits + k.hits) | 0;
  tagChars = (tagChars + k.tag.length) | 0;
  const p = k.peer;
  if (p !== null) {
    peerIds = (peerIds + p.id) | 0;
  }
  const n = k.note;
  if (n !== null) {
    notes = (notes + 1) | 0;
    noteSum = (noteSum + n.b) | 0;
  }
}

gc();
const mu = process.memoryUsage();

console.log("probe:13_large_eden_survivors");
console.log("checksum:" + checksum);
console.log("hits:" + hits);
console.log("tagChars:" + tagChars);
console.log("peerIds:" + peerIds);
console.log("notes:" + notes);
console.log("noteSum:" + noteSum);
console.error("#gcmetric heap_used_bytes=" + mu.heapUsed);
console.error("#gcmetric heap_total_bytes=" + mu.heapTotal);
console.error("#gcmetric rss_bytes=" + mu.rss);
