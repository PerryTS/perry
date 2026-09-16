// Isolates a defect this lane's RSS probe tripped over, so the RSS numbers can
// be read without wondering whether they are measuring it.
//
// N `worker_threads` Workers, each of which does nothing but `postMessage` and
// return. The parent counts the messages it receives. Node delivers N. Perry
// delivers N for N=1 and then becomes unreliable, on BOTH the base commit and
// this branch -- so it is a pre-existing defect, not a P9 regression, and this
// file is the smallest thing that says so.
//
//   P9_AGENTS=1 ./p9_worker_message_fanin   # expect got=1
//   P9_AGENTS=8 ./p9_worker_message_fanin   # expect got=8
//
// The watchdog is the point: the failure is a HANG, and a hang cannot be
// reported by a suite that is waiting for the process to finish.
import { Worker } from "node:worker_threads";

const agents = Number(process.env.P9_AGENTS ?? "4");
const budgetMs = Number(process.env.P9_BUDGET_MS ?? "10000");

let got = 0;
const workerUrl = new URL("./_helpers/p9_message_fanin_worker.ts", import.meta.url);
const workers: Worker[] = [];
for (let i = 0; i < agents; i++) {
  const w = new Worker(workerUrl);
  w.on("message", () => {
    got += 1;
  });
  w.on("error", () => {});
  workers.push(w);
}

const deadline = Date.now() + budgetMs;
while (got < agents && Date.now() < deadline) {
  await new Promise<void>((r) => setTimeout(r, 25));
}

console.log(`agents=${agents} got=${got} ${got === agents ? "OK" : "MISSING"}`);
process.exit(got === agents ? 0 : 1);
