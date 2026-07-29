// Test: repsel Phase 4a.3 — a PROMOTED (guard-free) numeric-array local whose
// backing storage is reallocated by push-driven growth must never be read
// through a stale head. Guard-free consumers have no runtime check that could
// catch a growth-forwarded stub, so this pins the cross-module invariant the
// eligibility proof depends on: every in-function growth site writes the live
// head back to the local slot, and every consumer re-derives the base from
// that slot per access.
//
// Each function below keeps its array fully contained (no bare references, no
// callee ever receives it), so the collector promotes it; growth then happens
// via `push` past the initial capacity, and the reads AFTER growth must see
// the relocated storage. Validated byte-for-byte against
// `node --experimental-strip-types`, flag on/off and under
// PERRY_GC_FORCE_EVACUATE=1.
export {};

// 1) Dense `[]` provenance: many growths (capacity doublings), bounded-loop
// reads after the last growth.
function growThenRead(n: number): number {
  const a: number[] = [];
  for (let i = 0; i < n; i++) {
    a.push(i * 0.5);
  }
  let sum = 0;
  for (let i = 0; i < a.length; i++) {
    sum += a[i] || 0;
  }
  return sum + a.length;
}
console.log(growThenRead(1));
console.log(growThenRead(2));
console.log(growThenRead(9)); // crosses the small-capacity boundary
console.log(growThenRead(1000));

// 2) Interleaved growth and reads: every read is preceded by a push that may
// have relocated the storage, so a cached head would surface immediately.
function interleaved(n: number): number {
  const a: number[] = [];
  let acc = 0;
  for (let i = 0; i < n; i++) {
    a.push(i + 0.25);
    acc += a[0] || 0; // element 0 after each (possibly relocating) push
    acc += a[i] || 0; // the element just pushed
  }
  return acc;
}
console.log(interleaved(1));
console.log(interleaved(64));
console.log(interleaved(513));

// 3) Growth followed by guard-free WRITES through the same binding, then
// reads: a stale head would write into freed storage.
function growWriteRead(n: number): number {
  const a: number[] = [];
  for (let i = 0; i < n; i++) {
    a.push(0);
  }
  for (let i = 0; i < a.length; i++) {
    a[i] = (a[i] || 0) + i * 2;
  }
  let sum = 0;
  for (let i = 0; i < a.length; i++) {
    sum += a[i] || 0;
  }
  return sum;
}
console.log(growWriteRead(3));
console.log(growWriteRead(300));

// 4) `new Array(n)` provenance with statically-in-bounds accesses, then growth
// past the allocation length via push, then reads of BOTH the original
// in-bounds region and the grown region.
function allocThenGrow(): string {
  const a: number[] = new Array(4);
  a[0] = 1.5;
  a[3] = 2.5;
  const beforeStatic = (a[0] || 0) + (a[3] || 0);
  for (let i = 0; i < 200; i++) {
    a.push(i * 0.125);
  }
  const afterStatic = (a[0] || 0) + (a[3] || 0); // same slots, relocated
  let tail = 0;
  for (let i = 0; i < a.length; i++) {
    tail += a[i] || 0;
  }
  return beforeStatic + " " + afterStatic + " " + tail + " " + a.length;
}
console.log(allocThenGrow());

// 5) Holes survive relocation: a `new Array(n)` local grown by push must keep
// its hole slots holey (JSON/`in` are observability surfaces, so they run on a
// separate NON-promoted array built from the same values).
function holesSurviveGrowth(): string {
  const a: number[] = new Array(3);
  a[1] = 7;
  for (let i = 0; i < 40; i++) {
    a.push(i);
  }
  const probe = (a[0] ?? -1) + "," + (a[1] || -1) + "," + (a[2] ?? -1) + "," + (a[42] || -1);
  return probe + " len=" + a.length;
}
console.log(holesSurviveGrowth());
