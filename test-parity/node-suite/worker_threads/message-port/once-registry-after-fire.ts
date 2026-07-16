import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
let fired = 0;

function count() {
  return typeof (port1 as any).listenerCount === "function"
    ? (port1 as any).listenerCount("message")
    : "unsupported";
}

function named() {
  return typeof (port1 as any).eventNames === "function"
    ? (port1 as any).eventNames().includes("message")
    : "unsupported";
}

port1.once("message", () => {
  fired += 1;
  console.log(
    "after fire:",
    fired,
    count(),
    named(),
  );
  port1.close();
  port2.close();
});
console.log("before:", count());
port2.postMessage("trigger");
