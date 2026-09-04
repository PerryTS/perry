// #9673: an `Array.prototype` higher-order method that binds the RAW callback
// pointer it was handed and keeps using it after the callback has allocated.
//
// A callback born at the call site — the inline arrow in `xs.forEach(x => …)` —
// is reachable ONLY through that raw parameter plus the native stack, which an
// evacuating minor does not scan. Every dispatch inside the loop allocates, so
// from some element on the loop dereferences a closure the collector has
// already retired. The read lands on recycled memory whose header is no longer
// `CLOSURE_MAGIC`; the next validation reports the recycled object's `typeof`
// instead — `TypeError: object is not a function`, which is what claude-code's
// OAuth login died with.
//
// `js_array_map` (#6081/#6206) and `js_array_filter` root the callback and
// re-read it at every dispatch, and `js_array_map_discard` got the same
// treatment when #7533's allocation change moved a collection into its window.
// forEach / some / every / find / findIndex / findLast / findLastIndex /
// flatMap / reduce were never converted and still bind the pre-collection
// address.
//
// PROOF INSTRUMENT: `PERRY_GC_PROTECT_FROMSPACE=1` mprotects retired
// from-space, so a stale use faults at the instruction that makes it — the same
// instrument that pinned the `map_discard` arm. On unfixed `origin/main` the
// `forEach` arm dies with
//
//   [gc-fromspace-protect] FAULT: signal 10 at 0x…
//     last-known object: user_ptr=0x… obj_type=4 size=32     (GC_TYPE_CLOSURE)
//   2  js_array_forEach + 4072
//
// and prints nothing. Node prints the arm's value; so must perry, with and
// without the instrument.
//
// One arm per process (argv[2]) so a fault attributes to exactly one method.
function junk(n: number): number {
  const a: any[] = [];
  for (let j = 0; j < n; j++) a.push({ k: j, s: "x" + j, arr: [j, j + 1, j + 2] });
  return a.length;
}
const src: number[] = [];
for (let i = 0; i < 4000; i++) src.push(i);
const which = process.argv[2] || "forEach";
let out: unknown;
switch (which) {
  case "forEach": { let n = 0; src.forEach((v) => { n += v + junk(60); }); out = n > 0; break; }
  case "some": out = src.some((v) => { junk(60); return v === 3999; }); break;
  case "every": out = src.every((v) => { junk(60); return v >= 0; }); break;
  case "find": out = src.find((v) => { junk(60); return v === 3999; }); break;
  case "findIndex": out = src.findIndex((v) => { junk(60); return v === 3999; }); break;
  case "findLast": out = src.findLast((v) => { junk(60); return v === 0; }); break;
  case "findLastIndex": out = src.findLastIndex((v) => { junk(60); return v === 0; }); break;
  case "flatMap": out = src.flatMap((v) => { junk(60); return v; }).length; break;
  case "reduce": out = src.reduce((a, v) => { junk(60); return a + v; }, 0); break;
  case "reduceRight": out = src.reduceRight((a, v) => { junk(60); return a + v; }, 0); break;
  case "map": out = src.map((v) => { junk(60); return v; }).length; break;
  case "filter": out = src.filter((v) => { junk(60); return v % 2 === 0; }).length; break;
  case "sort": out = src.slice(0, 800).sort((a, b) => { junk(60); return a - b; })[0]; break;
  // Controls: a genuine non-callable must still raise Node's exact message —
  // rooting the callback must not turn the TypeError into a silent no-op.
  case "bad": {
    const msgs: string[] = [];
    for (const bad of [{}, null, 5, "x"] as any[]) {
      try {
        src.map(bad);
        msgs.push("no throw");
      } catch (e: any) {
        msgs.push(e.message);
      }
    }
    out = msgs;
    break;
  }
  default: out = "unknown arm";
}
console.log(which + " -> " + JSON.stringify(out));
