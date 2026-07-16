import {
  MessageChannel,
  receiveMessageOnPort,
  Worker,
} from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const channel = new MessageChannel();
const worker = new Worker("./workerdata-port-alias-worker.cjs", {
  workerData: { left: channel.port1, right: channel.port1 },
  transferList: [channel.port1],
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
    "port alias:",
    summary?.leftBrand,
    summary?.rightBrand,
    summary?.alias,
  );
  console.log(
    "delivery:",
    receiveMessageOnPort(channel.port2)?.message,
    receiveMessageOnPort(channel.port2)?.message,
  );
  console.log("exit:", code);
  channel.port1.close();
  channel.port2.close();
});
