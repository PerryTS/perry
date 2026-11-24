// A scalar initializer cannot prove a module cell remains scalar after a
// callback writes it. Keep the evaluated argument alive after the cell is
// rebound, rather than re-reading the cell below the collection.
// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=121 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800 PERRY_GC_VERIFY_EVACUATION=1
declare function gc(): void;
// Escapes, so the loop allocates and keeps its back-edge collection poll.
let junk: any;
let saved = 0;
function publish(value: any): void { saved = value; }
function release(): number {
  saved = 0;
  if (typeof gc === "function") gc();
  for (let i = 0; i < 80; i++) { junk = {i}; }
  return 7;
}
function show(value: any, n: number): void { console.log(value.answer, n); }
publish({answer: 42});
show(saved, release());
console.log(saved);
