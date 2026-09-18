// turnloop P9 under GC stress: a worker agent's own loop, with the collector
// running at every handled safepoint and from-space quarantined.
//
//   PERRY_GC_DIAG=1 PERRY_GC_SCHEDULE_SEED=<n> PERRY_GC_SCHEDULE_RATE=1 \
//   PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1 \
//   PERRY_GC_PROTECT_FROMSPACE_DEPTH=800 PERRY_GC_FROMSPACE_SCAN_ABORT=1 \
//   PERRY_LOOP_STATS=1 ./p9_worker_gc_stress
//
// Byte-identical stdout is NOT on its own the verdict, and this lane was told
// so explicitly: a probe that retains an object graph across collections can
// validate its own output and still be measuring a corrupt heap — the same
// binary printed a byte-identical correct answer with 15,018 dangling
// references before a fix and 0 after. `PERRY_GC_FROMSPACE_SCAN_ABORT=1` (which
// implies `PERRY_GC_FROMSPACE_SCAN=1`) is the gate that actually catches that,
// and the `[gc-fromspace-protect] retired_set=#N` and `loop_polls=` counts are
// what say the instruments were armed at all.
//
// Two agents on purpose: the primary and a Worker do the same work at the same
// time, so a collection on one lands while the other has operations in flight.
import { Worker } from "node:worker_threads";

const url = process.env.P9_URL ?? "http://127.0.0.1:8099/";

function churn(retained: string[][]): number {
  let n = 0;
  for (let i = 0; i < 4000; i++) {
    const row = [`p9-${i}`, `row-${i}`, `${i * 3}`];
    if (i % 400 === 0) retained.push(row);
    n += row[0].length + row[1].length + row[2].length;
  }
  return n;
}

const workerUrl = new URL("./_helpers/p9_worker_gc_stress_worker.ts", import.meta.url);
const w = new Worker(workerUrl);
const workerDone = new Promise<void>((resolve) => {
  w.on("message", () => resolve());
  w.on("error", (e: Error) => {
    console.log("worker error:", e.message);
    resolve();
  });
});

const retained: string[][] = [];
const inflight = fetch(url);
let churned = churn(retained);
const res = await inflight;
const bytes = (await res.text()).length;
churned += churn(retained);

await workerDone;
await w.terminate();
churned += churn(retained);

let fingerprint = 0;
for (const row of retained) fingerprint += row[0].length + row[2].length;
console.log(
  `primary-agent gc-stress: status=${res.status} bytes=${bytes} ` +
    `retained=${retained.length} fingerprint=${fingerprint} churned=${churned}`,
);
console.log("done");
