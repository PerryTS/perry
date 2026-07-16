import {
  MessageChannel,
  receiveMessageOnPort,
  Worker,
} from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const first = new MessageChannel();
const second = new MessageChannel();
const worker = new Worker("./workerdata-multiple-ports-worker.cjs", {
  workerData: { first: first.port1, second: second.port1 },
  transferList: [first.port1, second.port1],
});
let summary: any;

worker.on("message", (message) => {
  summary = message;
});
worker.on(
  "error",
  (error) => console.log("error:", error.name, (error as any).code ?? ""),
);
worker.on("exit", (code) => {
  console.log(
    "ports:",
    summary?.firstBrand,
    summary?.secondBrand,
    summary?.distinct,
  );
  console.log(
    "delivery:",
    receiveMessageOnPort(first.port2)?.message,
    receiveMessageOnPort(second.port2)?.message,
  );
  console.log("exit:", code);
  first.port1.close();
  first.port2.close();
  second.port1.close();
  second.port2.close();
});
