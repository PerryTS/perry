// Isolates a defect this lane's RSS probe tripped over, so the RSS numbers can
// be read without wondering whether they are measuring it.
//
// N `worker_threads` Workers, each of which does nothing but `postMessage` and
// return. The parent counts the messages it receives. Node delivers N. Perry
// delivers N for N=1 and then becomes unreliable, on BOTH the base commit and
// this branch -- so it is a pre-existing defect, not a P9 regression, and this
// file is the smallest thing that says so.
//
//   P9_FANIN_MODE=poll    P9_AGENTS=8 ./p9_worker_message_fanin
//   P9_FANIN_MODE=promise P9_AGENTS=8 ./p9_worker_message_fanin
//
// The two modes differ in ONE thing: whether the parent has a timer pending
// while it waits. `poll` does; `promise` awaits `Promise.all` over the message
// events and has nothing else pending at all. The failure is a HANG, so it
// cannot be reported by a suite that is waiting for the process to finish --
// bound it with `timeout` and read the exit code.
import { Worker } from "node:worker_threads";

const agents = Number(process.env.P9_AGENTS ?? "4");
const budgetMs = Number(process.env.P9_BUDGET_MS ?? "10000");
// "poll": the parent keeps a 25 ms timer pending while it waits.
// "promise": the parent awaits `Promise.all` over the message events and has
//            NOTHING else pending -- no timer, no handle of its own. That is
//            the shape that hangs, and the difference between the two is the
//            whole finding.
const mode = process.env.P9_FANIN_MODE ?? "poll";

const workerUrl = new URL("./_helpers/p9_message_fanin_worker.ts", import.meta.url);
const workers: Worker[] = [];
const waits: Promise<void>[] = [];
let got = 0;
for (let i = 0; i < agents; i++) {
  const w = new Worker(workerUrl);
  waits.push(
    new Promise<void>((resolve) => {
      w.on("message", () => {
        got += 1;
        resolve();
      });
      w.on("error", () => resolve());
    }),
  );
  workers.push(w);
}

if (mode === "promise") {
  // No watchdog is possible here without adding the very timer under test, so
  // this arm is bounded by the caller's `timeout` and a hang shows up as exit
  // 124 with no output at all. That IS the observation.
  await Promise.all(waits);
} else {
  const deadline = Date.now() + budgetMs;
  while (got < agents && Date.now() < deadline) {
    await new Promise<void>((r) => setTimeout(r, 25));
  }
}

console.log(`agents=${agents} mode=${mode} got=${got} ${got === agents ? "OK" : "MISSING"}`);

// The teardown is its own case. `P9_FANIN_TERMINATE=1` awaits `terminate()` on
// every worker and prints a second line; a run that prints the first line and
// then dies to the caller's `timeout` has located the hang in the teardown, not
// in the fan-in. That distinction matters: buffered stdout makes a teardown hang
// look like a fan-in hang, because the row that WAS computed never reaches the
// terminal.
if (process.env.P9_FANIN_TERMINATE === "1") {
  for (const w of workers) await w.terminate();
  console.log("terminated all");
}

process.exit(got === agents ? 0 : 1);
