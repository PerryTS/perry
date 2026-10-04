// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=11821 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1
// #11821: `p.finally(cb)` settles with p's own outcome even when `cb` (user
// code) and the continuation allocations move it. Covers a cleanup that
// returns a promise, one that returns nothing, and a rejection.
const sink: string[] = [];
function churn(): void {
  let s = "";
  for (let i = 0; i < 40; i++) s += `x${i}`;
  sink.push(s);
  if (sink.length > 50) sink.length = 0;
}
async function produce(n: number): Promise<{ packages: Record<string, { v: string }> }> {
  await null;
  const packages: Record<string, { v: string }> = {};
  for (let i = 0; i < 20; i++) packages[`p${i}`] = { v: `${n}.${i}` };
  return { packages };
}
async function fail(n: number): Promise<never> {
  await null;
  throw new Error(`boom ${n}`);
}
let out = "";
for (let k = 0; k < 10; k++) {
  const a = await produce(k).finally(() => { churn(); return Promise.resolve().then(churn); });
  const b = await produce(k).finally(() => { churn(); });
  let reason = "";
  try {
    await fail(k).finally(() => { churn(); return Promise.resolve(); });
  } catch (e) {
    reason = (e as Error).message;
  }
  out = `${Object.values(a.packages).map((p) => p.v).join(",").length} ${b.packages.p19!.v} ${reason}`;
}
console.log(out);
