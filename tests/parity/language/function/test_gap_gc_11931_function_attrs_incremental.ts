// parity-env: PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_SCHEDULE_SEED=11931 PERRY_GC_SCHEDULE_RATE=0.2 PERRY_GC_SCHEDULE_ALLOC_KB=64
// Additional owner matrix settings: PERRY_GC_BUDGETED_OLD_RECLAIM=1 PERRY_GC_PROTECT_OLD_SWEEP=1 PERRY_GC_MAJOR_PACING_FLOOR_MB=1 PERRY_GC_MAJOR_PACING_GROWTH=1
// Allocate born-old buffers and yield between function attribute changes.
// The harness asserts a completed budgeted full cycle and quarantine banner.
const fn: any = Math.min;
const kept: any[] = [];
let trash: any[] = [];
let failures = 0;
async function step(n: number): Promise<void> {
  trash = [];
  for (let j = 0; j < 8; j++) trash.push({n, j, text: "trash-" + j});
  const bytes = new Uint8Array(20000);
  bytes[0] = n % 251;
  kept.push(bytes);
  if (kept.length > 48) kept.shift();
  Object.defineProperty(fn, "name", {value: "cycle-" + n, writable: true, enumerable: n % 2 === 0, configurable: true});
  Object.defineProperty(fn, "length", {value: n % 7, writable: n % 2 === 0, enumerable: true, configurable: true});
  Object.defineProperty(fn, "cycleExtra", {value: "extra-" + n, writable: true, enumerable: true, configurable: true});
  await null;
  const name: any = Object.getOwnPropertyDescriptor(fn, "name");
  const length: any = Object.getOwnPropertyDescriptor(fn, "length");
  if (fn.name !== "cycle-" + n || name.value !== fn.name || !name.writable || name.enumerable !== (n % 2 === 0) || !name.configurable) failures++;
  if (fn.length !== n % 7 || length.value !== fn.length || length.writable !== (n % 2 === 0) || !length.enumerable || !length.configurable) failures++;
  if (fn.cycleExtra !== "extra-" + n) failures++;
  if (n + 1 < 6000) return step(n + 1);
}
step(0).then(() => { console.log("incremental-function-attributes", failures, kept.length, trash.length, fn.name, fn.length, fn.cycleExtra); });
