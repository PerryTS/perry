// #8692 moving-GC witness for the guarded direct Uint32Array RMW.  Keep this
// workload deliberately small: scheduling a collection at every opportunity
// over the ticket's 2,000,000-update performance ratchet would turn a focused
// correctness arm into a minutes-long parity test.
// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=8692 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_PROTECT_FROMSPACE=1

const values = new Uint32Array(32);

function allocatingOne(round: number, key: number): number {
  // Unary `+` at the call site makes the RHS a proven Number while retaining
  // this allocating call between the direct element load and the post-RHS
  // receiver reload/guard.
  const churn: object[] = [];
  for (let i = 0; i < 8; i++) {
    churn.push({ round, key, i, text: ("gc-" + round + "-" + key + "-" + i).repeat(4) });
  }
  return 1;
}

function bump(target: Uint32Array, key: any, round: number): void {
  target[key] += +allocatingOne(round, key);
}

for (let round = 0; round < 8; round++) {
  for (let i = 0; i < values.length; i++) bump(values, i, round);
}

console.log("moving-rmw", values[0], values[31]);
