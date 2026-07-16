import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const methods = [
  "addListener",
  "emit",
  "eventNames",
  "listenerCount",
  "listeners",
  "off",
  "on",
  "once",
  "prependListener",
  "prependOnceListener",
  "rawListeners",
  "removeAllListeners",
  "removeListener",
];

console.log(
  "methods:",
  methods.map((name) => `${name}:${typeof (port1 as any)[name]}`).join(","),
);
console.log(
  "event target:",
  typeof port1.addEventListener,
  typeof port1.removeEventListener,
  typeof port1.dispatchEvent,
);

const eventNames = typeof (port1 as any).eventNames === "function"
  ? JSON.stringify((port1 as any).eventNames())
  : "unsupported";
console.log("event names:", eventNames);

port1.close();
port2.close();
