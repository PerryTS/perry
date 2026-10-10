// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_SCHEDULE_SEED=41 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0
// A byte-view proof must compare the current rooted receiver after a call,
// including the identity carried around an allocating loop.
let sum = 0;
for (let i = 0; i < 80; i++) {
  const view = new Int32Array([i, i + 1, i + 2]);
  for (let j = 0; j < 3; j++) {
    sum += view[j];
    const churn = [{value: i}, {value: j}];
    sum += churn[1].value;
  }
}
console.log(sum);
