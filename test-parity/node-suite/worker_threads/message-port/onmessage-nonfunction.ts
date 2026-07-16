import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const assigned = { not: "callable" };

port1.onmessage = () => console.log("unexpected handler");
console.log(
  "before:",
  typeof port1.onmessage,
  port1.hasRef(),
  typeof (port1 as any).listenerCount === "function"
    ? (port1 as any).listenerCount("message")
    : "unsupported",
);

(port1 as any).onmessage = assigned;
console.log(
  "after:",
  port1.onmessage === assigned,
  port1.hasRef(),
  typeof (port1 as any).listenerCount === "function"
    ? (port1 as any).listenerCount("message")
    : "unsupported",
);

port1.close();
port2.close();
