// Bug 9: a WeakMap entry keyed by a native handle must die with its key. The
// collector recycles an unreachable handle's id, so a later handle can get
// the same id; the entry keyed by the old handle must be gone by then, or the
// new handle answers with the old value. An entry whose value names its own
// key (`wm.set(d, { d })`) is a cycle through the WeakMap, not an owner: the
// key must still be collected, so the loop never runs out of ids.
import { StringDecoder } from "node:string_decoder";

const wm = new WeakMap<object, string>();
function seed(n: number) {
  for (let i = 0; i < n; i++) wm.set(new StringDecoder("utf8"), "seed " + i);
}
seed(64);

let hits = 0;
for (let i = 0; i < 400000; i++) {
  const d = new StringDecoder("utf8");
  if (wm.get(d) !== undefined || wm.has(d)) hits++;
}
console.log("stale hits", hits);

const cyc = new WeakMap<object, { d: object }>();
let cycHits = 0;
for (let i = 0; i < 400000; i++) {
  const d = new StringDecoder("utf8");
  if (cyc.has(d)) cycHits++;
  cyc.set(d, { d });
}
console.log("self-cycle entries", "stale hits", cycHits);

const kept = new StringDecoder("utf8");
wm.set(kept, "kept");
for (let i = 0; i < 300000; i++) new StringDecoder("utf8");
console.log("kept entry", wm.get(kept));
