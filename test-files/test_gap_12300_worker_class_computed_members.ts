// #12300: a worker_threads Worker evaluates its own module graph, so its
// classes' computed members (string, well-known, registered and unique
// symbol keys, accessors) are named by its own evaluation: the worker sees
// every member, in ClassBody order, exactly like the main thread.
import { Worker } from "node:worker_threads";
import { C, exercise } from "./_helpers/class_computed_members_12300.ts";
console.log("main " + exercise(new C()));
const w = new Worker(new URL("./_helpers/class_computed_members_12300_worker.ts", import.meta.url));
w.on("message", (d: string) => {
  console.log(d);
  w.terminate().then(() => process.exit(0));
});
setTimeout(() => process.exit(2), 15000);
