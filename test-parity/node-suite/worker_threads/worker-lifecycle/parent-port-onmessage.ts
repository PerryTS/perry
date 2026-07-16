import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./parent-port-onmessage-worker.cjs");
const values: string[] = [];
let valuesLogged = false;

worker.on("message", (message) => {
  values.push(message === undefined ? "undefined" : JSON.stringify(message));
  if (values.length === 4) {
    console.log("values:", values.join(","));
    valuesLogged = true;
    worker.terminate().then((code) => console.log("terminate:", code));
  }
});
worker.on("exit", (code) => {
  if (!valuesLogged) {
    console.log("values: missing");
  }
  console.log("exit:", code);
});
worker.postMessage(2);
