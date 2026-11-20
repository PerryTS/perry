// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=11816 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1
// #11816: filter / find / findLast hand back the element the callback saw. A
// callback that allocates can run an evacuating minor that moves a young
// element (an `Object.entries` pair); the result must hold the moved element,
// not its pre-collection address. The seeded schedule collects at the
// callback's allocations, and from-space protection faults on a stale read.
function allocating(v: string): boolean {
  let s = "";
  for (let i = 0; i < 20; i++) s += `${v}:${i};`;
  return s.length > 0 && v.endsWith("0");
}
function run(round: number): string {
  const versions: Record<string, { v: string }> = {};
  for (let i = 0; i < 40; i++) versions[`${round}.${i}.0`] = { v: `${round}.${i}.0` };
  const kept = Object.entries(versions).filter(([v]) => allocating(v));
  const found = Object.entries(versions).find(([v]) => allocating(v) && v === `${round}.30.0`);
  const last = Object.entries(versions).findLast(([v]) => allocating(v));
  let sum = 0;
  for (const [k, m] of kept) sum += k === m.v ? 1 : 0;
  return `${kept.length} ${sum} ${kept[0]![1].v} ${found![1].v} ${last![1].v}`;
}
for (let round = 0; round < 3; round++) console.log(run(round));
