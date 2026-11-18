// #10394: an async parentPort 'message' handler resumes after each await and
// posts its reply, whether it waited on a job, a timer or a promise-based
// fs call. Each request is sent only after the previous reply arrived, so
// the order is fixed; the guard timer only turns a lost reply into output.
import { Worker } from "node:worker_threads";

const w = new Worker(new URL("./fixtures/async_message_handler_worker.ts", import.meta.url));
const replies: string[] = [];
let waiting: ((via: string) => void) | null = null;
w.on("message", (via: string) => {
  const resolve = waiting;
  waiting = null;
  if (resolve) resolve(via);
});
const guard = setTimeout(() => {
  console.log("timeout after", JSON.stringify(replies));
  process.exit(1);
}, 10000);
const call = (kind: string) =>
  new Promise<string>((resolve) => {
    waiting = resolve;
    w.postMessage(kind);
  });
for (const kind of ["sync", "microtask", "timer", "timer2", "io"]) {
  replies.push(kind + " -> " + (await call(kind)));
}
clearTimeout(guard);
console.log(replies.join("\n"));
await w.terminate();
