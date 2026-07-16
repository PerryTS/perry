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

worker.on("message", (message) => {
  console.log(
    "port alias:",
    message.leftBrand,
    message.rightBrand,
    message.alias,
  );
  console.log(
    "delivery:",
    receiveMessageOnPort(channel.port2)?.message,
    receiveMessageOnPort(channel.port2)?.message,
  );
});
worker.on(
  "error",
  (error) => console.log("error:", error.name, (error as any).code ?? ""),
);
worker.on("exit", (code) => {
  console.log("exit:", code);
  channel.port1.close();
  channel.port2.close();
});
