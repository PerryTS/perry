import { MessageChannel, Worker } from "node:worker_threads";

function summarize(label: string, prototype: object, names: string[]) {
  for (const name of names) {
    const descriptor = Object.getOwnPropertyDescriptor(prototype, name);
    console.log(
      label,
      name,
      Boolean(descriptor),
      typeof descriptor?.value,
      typeof descriptor?.get,
      descriptor?.enumerable,
    );
  }
}

summarize("Worker", Worker.prototype, [
  "postMessage",
  "terminate",
  "ref",
  "unref",
  "threadId",
  "threadName",
  "resourceLimits",
  "stdin",
  "stdout",
  "stderr",
  "performance",
]);

const { port1, port2 } = new MessageChannel();
summarize("MessagePort", Object.getPrototypeOf(port1), [
  "postMessage",
  "start",
  "close",
  "ref",
  "unref",
  "hasRef",
  "onmessage",
  "onmessageerror",
]);
port1.close();
port2.close();
