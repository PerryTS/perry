import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

let arrayBuffer: any;
let sharedBuffer: any;
let detachedLength: any = "missing";
const worker = new Worker("./worker-created-buffers-worker.cjs");

worker.on("message", (value: any) => {
  if ("detachedLength" in value) {
    detachedLength = value.detachedLength;
    return;
  }
  arrayBuffer = value.arrayBuffer;
  sharedBuffer = value.sharedBuffer;
});
worker.on("error", (error: any) => {
  console.log("error:", error?.name, error?.code ?? "");
});
worker.on("exit", (code) => {
  const arrayValues = arrayBuffer instanceof ArrayBuffer
    ? Array.from(new Uint8Array(arrayBuffer)).join(",")
    : "missing";
  const sharedValues = sharedBuffer instanceof SharedArrayBuffer
    ? Array.from(new Uint8Array(sharedBuffer)).join(",")
    : "missing";
  console.log(
    "brands:",
    arrayBuffer instanceof ArrayBuffer,
    sharedBuffer instanceof SharedArrayBuffer,
  );
  console.log("values:", arrayValues, sharedValues, detachedLength);
  console.log("exit:", code);
});
