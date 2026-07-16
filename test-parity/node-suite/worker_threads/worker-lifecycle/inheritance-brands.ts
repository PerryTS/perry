import { MessageChannel, MessagePort, Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const { port1, port2 } = new MessageChannel();
console.log(
  "main port:",
  port1 instanceof MessagePort,
  port1 instanceof EventTarget,
);
port1.close();
port2.close();

const worker = new Worker("./inheritance-brands-worker.cjs");
console.log("worker:", worker instanceof EventTarget);
worker.on("message", (message: any) => {
  console.log(
    "parent port:",
    message.messagePort,
    message.eventTarget,
  );
});
worker.on("exit", (code) => console.log("exit:", code));
