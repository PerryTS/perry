// parity-env: PERRY_GC_BUDGETED_OLD_RECLAIM=1 PERRY_GC_MAJOR_PACING_FLOOR_MB=1 PERRY_GC_MAJOR_PACING_GROWTH=1
// #11842: a swept old-generation hole handed out while a budgeted full sweep
// is still walking the heap.
//
// A budgeted (incremental) full collection sweeps across mutator windows, and
// an object born in one of those windows carries no mark. A buffer above the
// 16 KiB born-old threshold is allocated in the old generation, and a
// same-size hole left by the previous sweep is reused before the bump pointer
// moves. When that hole lay in a block the in-flight sweep had not reached
// yet, the sweep found the new buffer unmarked and freed it while the program
// still held it. The next same-size buffer then took the same bytes, so two
// live buffers shared memory. A compiled package manager wrote wrong files,
// lost resolved packages and crashed in `crypto.hash` on a "string" 1.4 GB
// long.
//
// `PERRY_GC_BUDGETED_OLD_RECLAIM=1` makes every old-gen collection a budgeted
// full that runs at the microtask pump, so the `await` after each allocation
// is where its mark and sweep steps run. Every buffer has one size, so every
// reuse is an exact fit; a sliding window of them stays alive while the rest
// die, and each kept buffer is filled with its own byte and checked. With the
// bug a buffer freed while live is overwritten by a newer one.

const SIZE = 20000;
const WINDOW = 192;
const STEPS = 20000;

const live: Uint8Array[] = [];
const tags: number[] = [];
let mismatches = 0;
let firstBad = "";

// Young garbage between the buffers, built by the runtime rather than a loop.
const junkText = JSON.stringify(
  Array.from({ length: 300 }, (_, i) => ({ id: i, name: "pkg" + i, deps: ["a", "b", "c"] })),
);
let keepJunk: unknown[] = [];

function check(at: number): void {
  for (let k = 0; k < live.length; k++) {
    const b = live[k];
    const t = tags[k];
    if (b[0] !== t || b[SIZE >> 1] !== t || b[SIZE - 1] !== t) {
      mismatches++;
      if (firstBad === "") {
        firstBad = `step ${at} slot ${k}: want ${t}, got ${b[0]}/${b[SIZE >> 1]}/${b[SIZE - 1]}`;
      }
    }
  }
}

async function step(n: number): Promise<void> {
  const tag = n % 251;
  const bytes = new Uint8Array(SIZE);
  bytes.fill(tag);
  if (n % 3 === 0) {
    live.push(bytes);
    tags.push(tag);
    if (live.length > WINDOW) {
      live.shift();
      tags.shift();
    }
  }
  const junk = JSON.parse(junkText);
  if (n % 50 === 0) keepJunk.push(junk);
  if (keepJunk.length > 40) keepJunk = [];
  if (n % 64 === 0) check(n);
  // A microtask turn is a runtime safepoint: budgeted collection steps run
  // here, between the allocations.
  await null;
  if (n + 1 < STEPS) return step(n + 1);
}

step(0).then(() => {
  check(STEPS);
  console.log(`kept ${live.length} buffers of ${SIZE} bytes`);
  console.log(`mismatches: ${mismatches}`);
  if (firstBad !== "") console.log(`first: ${firstBad}`);
});
