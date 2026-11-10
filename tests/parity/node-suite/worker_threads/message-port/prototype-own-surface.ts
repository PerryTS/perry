import { MessageChannel, MessagePort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
console.log(
  "own names:",
  Object.getOwnPropertyNames(MessagePort.prototype).sort().join(","),
);
console.log(
  "prototype identity:",
  Object.getPrototypeOf(port1) === MessagePort.prototype,
);

const event = new MessageEvent("message");
console.log(
  "event brands:",
  event instanceof MessageEvent,
  event instanceof Event,
  Object.getPrototypeOf(event) === MessageEvent.prototype,
);

port1.close();
port2.close();
