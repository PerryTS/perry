import { Worker } from "node:worker_threads";
import { performance as imported } from "node:perf_hooks";
import { sampleTiming } from "./_helpers/profiling_api_helper.ts";
const direct = performance.timeOrigin + performance.now();
const event = { t: performance.timeOrigin + performance.now() };
console.log("global finite", Number.isFinite(direct));
console.log("object finite", Number.isFinite(event.t));
const main = sampleTiming();
console.log("helper", main.finite, main.monotonic, main.plausible, main.identity, main.stable);
console.log("import identity", imported === performance);
// A local must take precedence over the global's special call lowering.
function shadow(performance: { now: () => number, timeOrigin: number }) {
  return performance.timeOrigin + performance.now();
}
console.log("shadow", shadow({ timeOrigin: 40, now: () => 2 }) === 42);
const guard = setTimeout(() => { console.log("timeout"); process.exit(2); }, 10000);
const w = new Worker(new URL("./_helpers/profiling_api_worker.ts", import.meta.url));
w.once("message", (reply: any) => {
  const t = reply.timing;
  console.log("worker", t.finite, t.monotonic, t.plausible, t.identity, t.stable);
  console.log("shared origin", t.origin === main.origin);
  clearTimeout(guard);
});
