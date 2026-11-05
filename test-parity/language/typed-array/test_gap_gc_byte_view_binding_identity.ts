// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_SCHEDULE_SEED=41 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0
// Pure initialization must invalidate an immutable byte-view proof on
// every lexical binding. There is no collecting call in the critical loops.
const a = new Uint32Array([7]);
const b = new Uint32Array([19]);
const views = [a, b];
let sum = 0;
for (let i = 0; i < 80; i++) {
  const view: Uint32Array = views[i & 1];
  for (let j = 0; j < 3; j++) sum += view[0];
}
// Separately require the stress arm to actually move ordinary objects.
let churnSum = 0;
for (let i = 0; i < 60; i++) {
  const churn = [{value: i}, {value: i + 1}];
  churnSum += churn[1].value;
}
console.log(sum, churnSum, a[0], b[0]);
