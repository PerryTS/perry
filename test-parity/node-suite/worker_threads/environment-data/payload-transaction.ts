import { setEnvironmentData, Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/environment-data");
const buffer = new Uint8Array([9, 8, 7]).buffer;
setEnvironmentData("invalid", () => {});
try {
  new Worker("./payload-transaction-worker.cjs", {
    workerData: buffer,
    transferList: [buffer],
  });
  console.log("unexpected construction");
} catch (error: any) {
  console.log("rejected:", error.name, error.code);
}
console.log("ownership:", buffer.byteLength, Array.from(new Uint8Array(buffer)).join(","));
setEnvironmentData("invalid", undefined);
let environmentReads = 0;
setEnvironmentData("order", {
  get value() {
    environmentReads++;
    throw new Error("ENV_READ");
  },
});
try {
  new Worker("./payload-transaction-worker.cjs", {
    workerData: () => 1,
    transferList: [buffer],
  });
} catch (error: any) {
  console.log("precedence:", error.name, error.code, environmentReads, buffer.byteLength);
}
setEnvironmentData("order", undefined);
let reads = 0;
let count = 1;
const shared = { get count() { reads++; return count; } };
setEnvironmentData("alias", shared);
const worker = new Worker("./payload-transaction-worker.cjs", { workerData: shared });
console.log("getter reads:", reads);
count = 9;
worker.on("message", (message) => console.log("snapshot:", message.same, message.count));
worker.on("exit", (code) => console.log("exit:", code));
