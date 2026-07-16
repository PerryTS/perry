import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const shared = { value: 7 };
const buffer = new ArrayBuffer(4);
const view = new Uint16Array(buffer);
view[0] = 0x1234;

const worker = new Worker("./workerdata-alias-view-worker.cjs", {
  workerData: {
    left: shared,
    right: shared,
    buffer,
    view,
  },
});
console.log("parent:", buffer.byteLength, view.byteLength, view[0]);
worker.on("message", (message) => {
  console.log(
    "worker:",
    message.alias,
    message.viewBrand,
    message.bufferBrand,
    message.backing,
    message.value,
    message.lengths.join(","),
  );
});
worker.on(
  "error",
  (error) => console.log("error:", error.name, (error as any).code ?? ""),
);
worker.on("exit", (code) => console.log("exit:", code));
