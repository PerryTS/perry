// parity-env: PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_SCHEDULE_SEED=11931 PERRY_GC_SCHEDULE_RATE=0.2 PERRY_GC_SCHEDULE_ALLOC_KB=64
// Additional owner matrix settings: PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_VERIFY_MARK=1
// Re-key an aged builtin bag, then retain its descriptors through minors.
// The runtime companion asserts the old/young generations explicitly.
declare function gc(): void;
function collect(): void { if (typeof gc === "function") gc(); }
function churn(): void {
  const junk: any[] = [];
  for (let i = 0; i < 512; i++) junk.push({ value: "junk-" + i });
}
const funcs: any[] = [Object.keys, Math.max, Array.isArray];
for (let i = 0; i < 8; i++) { churn(); collect(); }
for (let i = 0; i < funcs.length; i++) {
  const fn = funcs[i];
  Object.defineProperty(fn, "name", {value: "aged-" + i, writable: true, enumerable: true, configurable: true});
  Object.defineProperty(fn, "length", {value: 20 + i, writable: true, enumerable: true, configurable: true});
  Object.defineProperty(fn, "youngExtra", {value: "extra-" + i, writable: false, enumerable: true, configurable: true});
}
churn(); collect();
for (let i = 0; i < funcs.length; i++) {
  const fn = funcs[i];
  for (const key of ["name", "length", "youngExtra"]) {
    const d: any = Object.getOwnPropertyDescriptor(fn, key);
    console.log("aged", i, key, JSON.stringify([d.value, d.writable, d.enumerable, d.configurable]));
  }
  console.log("aged-read", i, fn.name, fn.length, fn.youngExtra, Object.keys(fn).join(","));
}
