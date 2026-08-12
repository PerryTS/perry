// GC ratchet probe: large live set held across many nursery collections.
//
// Every earlier probe holds a small live set, which is exactly how the
// survivor-saturation regression shipped: with a live set big enough to
// saturate the survivor semispaces, a fixed tenuring age re-copies the same
// survivor cohort on every collection (measured 4.39 GB of copying for a
// ~35 MB live set on tree.ts). This probe grows a ~100 MB linked structure
// while transient garbage keeps collections firing, so the copied/promoted
// counters and peak RSS pin the large-live-set steady state; the release
// phase then pins that a promoted-then-dropped cohort is actually
// reclaimed, not stranded in old-gen.

declare function gc(): void;

class LNode {
  next: LNode | null;
  v: number;
  constructor(v: number) {
    this.next = null;
    this.v = v;
  }
}

const KEEP = 700000;
const KEEP_EVERY = 64;

const all: LNode[] = [];
let head: LNode | null = null;
let checksum = 0;
for (let i = 0; i < KEEP; i++) {
  const n = new LNode(i);
  n.next = head;
  head = n;
  all.push(n);
  // Transient garbage between live allocations so nursery collections keep
  // firing while the live set grows — the survivor influx stays heavy for
  // the whole build phase instead of arriving in one burst.
  const t1 = { x: i, y: i * 2 };
  const t2 = { x: i + 1, y: i * 3 };
  const t3 = { x: i + 2, y: i * 5 };
  checksum = (checksum + t1.x + t2.y + t3.y) | 0;
}

// Walk the full chain: every `next` pointer has crossed many collections,
// survivor copies, promotions, and old-gen reference rewrites by now.
let cur: LNode | null = head;
let walked = 0;
let sum = 0;
while (cur !== null) {
  walked = (walked + 1) | 0;
  sum = (sum + cur.v) | 0;
  cur = cur.next;
}

// Release phase: keep one node in KEEP_EVERY with its chain link severed,
// drop everything else. The retained-heap metric after the explicit gc()
// fails if the promoted bulk is stranded instead of reclaimed.
const kept: LNode[] = [];
for (let i = 0; i < all.length; i += KEEP_EVERY) {
  const n = all[i];
  n.next = null;
  kept.push(n);
}
all.length = 0;
head = null;
cur = null;

gc();

let keptSum = 0;
for (let i = 0; i < kept.length; i++) {
  keptSum = (keptSum + kept[i].v) | 0;
}

const mu = process.memoryUsage();

console.log("probe:12_large_live_set");
console.log("checksum:" + checksum);
console.log("walked:" + walked);
console.log("sum:" + sum);
console.log("kept:" + kept.length);
console.log("keptSum:" + keptSum);
console.error("#gcmetric heap_used_bytes=" + mu.heapUsed);
console.error("#gcmetric heap_total_bytes=" + mu.heapTotal);
console.error("#gcmetric rss_bytes=" + mu.rss);
