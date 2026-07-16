import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./termination-worker.cjs");
const names = [
  "addListener",
  "emit",
  "eventNames",
  "getMaxListeners",
  "listenerCount",
  "listeners",
  "off",
  "on",
  "once",
  "prependListener",
  "prependOnceListener",
  "rawListeners",
  "removeAllListeners",
  "removeListener",
  "setMaxListeners",
];

console.log(
  "methods:",
  names.map((name) => `${name}:${typeof (worker as any)[name]}`).join(","),
);
const eventNames = typeof (worker as any).eventNames === "function"
  ? JSON.stringify((worker as any).eventNames())
  : "unsupported";
console.log("event names:", eventNames);

if (
  typeof (worker as any).getMaxListeners === "function" &&
  typeof (worker as any).setMaxListeners === "function"
) {
  console.log(
    "max listeners:",
    typeof (worker as any).getMaxListeners(),
    (worker as any).setMaxListeners(7) === worker,
  );
  console.log("updated max:", (worker as any).getMaxListeners());
} else {
  console.log("max listeners: unsupported");
}

worker.terminate().then((code) => console.log("terminate:", code));
worker.on("exit", (code) => console.log("exit:", code));
