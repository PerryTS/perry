// #8423: both of these operations used to take a raw pointer into a source
// string's inline payload and then allocate the destination before copying it.
// Keep the two paths under allocation pressure together so moving alloc-point
// collections can exercise the source-rooting contract as that GC path evolves.
// parity-env: PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_PROTECT_FROMSPACE=1

const astral = "🦆";

let badWellFormed = 0;
let badClone = 0;
// Carry the source across the loop poll: an iteration-local source is dead
// before that poll, so the collector can reclaim every string without moving one.
let source = `well-formed-${astral}-0-`.repeat(16) + "tail";
for (let i = 0; i < 1000; i++) {
  if (source.toWellFormed() !== source) badWellFormed++;
  if (structuredClone(source) !== source) badClone++;
  // This fresh, non-SSO copy source must survive until the next iteration.
  source = `well-formed-${astral}-${i + 1}-`.repeat(16) + "tail";
}

console.log("toWellFormed copies:", badWellFormed === 0);
console.log("structuredClone copies:", badClone === 0);
