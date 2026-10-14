// ehfree slice 1 (design §5 test 7): a worker's base trap is a landing pad
// like every other handler. An uncaught throw in a worker (from its module
// body and from a timer callback, which hands it to the base past the timer
// drain's own trap) ends only that worker with an `error` event; a worker
// that throws and catches thousands of times keeps working. (`terminate()` of
// a worker spinning in a call-free loop does not stop it on main either — a
// cooperative-termination gap tracked separately.)
import { Worker } from "node:worker_threads";

const url = new URL("./_helpers/try_free_entry_worker.ts", import.meta.url);

function run(role: string): Promise<void> {
  return new Promise((resolve) => {
    const w = new Worker(url, { workerData: role });
    const events: string[] = [];
    w.on("message", (m: string) => {
      events.push("message " + m);
    });
    w.on("error", (e: Error) => events.push("error " + e.message));
    w.on("exit", (code: number) => {
      console.log(`7 ${role}: ${events.join("; ")}; exit ${code}`);
      resolve();
    });
  });
}

for (const role of ["throw", "timer", "caught"]) {
  await run(role);
}
console.log("7 main still runs");
