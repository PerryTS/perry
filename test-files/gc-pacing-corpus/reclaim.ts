// Old-generation reclaim control: bounded live data, continuously replaced.
// Large pointer-free strings allocate directly in old-gen (arena's general
// large-object route). No manual gc(), lowered threshold, or schedule forcing.
// Unlike retain.ts, old data becomes garbage on every replacement. Run with
// PERRY_GC_DIAG=1 and scripts/gc_reclaim_check.py; completion alone proves nothing.
declare const process: any;
const SLOTS = Number(process.argv[2] || 64);
const ROUNDS = Number(process.argv[3] || 6);
const BYTES = 1024 * 1024;
const live: string[] = [];
// Keep the last allocation observable, while all previous nursery objects die.
// Without this churn, empty-young minors classify the heap as retaining even
// though the replaced OLD strings are dead; that is the wrong pacing regime.
let garbage: any = null;
let checksum = 0;
for (let round = 0; round < ROUNDS; round++) {
  for (let i = 0; i < SLOTS; i++) {
    for (let j = 0; j < 2048; j++) {
      garbage = { a: j, b: i, c: round, d: j + 1 };
    }
    checksum += garbage.a;
    const code = 65 + ((round + i) % 26);
    live[i] = String.fromCharCode(code).repeat(BYTES);
    checksum += live[i].length + live[i].charCodeAt(BYTES - 1);
  }
}
// Read the complete final live set so premature reclamation is observable.
for (let i = 0; i < SLOTS; i++) {
  checksum += live[i].length + live[i].charCodeAt(0);
}
console.log("reclaim", SLOTS, ROUNDS, checksum);
