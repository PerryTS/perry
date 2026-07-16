import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const buffer = new ArrayBuffer(8);
new Uint8Array(buffer)[0] = 9;
const worker = new Worker("./natural-exit-worker.cjs", {
  transferList: [buffer],
});

console.log("detached:", buffer.byteLength);
worker.on(
  "error",
  (error: any) => console.log("error:", error?.name, error?.code ?? ""),
);
worker.on("exit", (code) => console.log("exit:", code));
