import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
let calls = 0;
const listener = () => calls += 1;

port1.once("message", listener);
const before = typeof (port1 as any).listenerCount === "function"
  ? (port1 as any).listenerCount("message")
  : "unsupported";
port1.off("message", listener);
const after = typeof (port1 as any).listenerCount === "function"
  ? (port1 as any).listenerCount("message")
  : "unsupported";

port1.on("message", () => {
  console.log("counts:", before, after, calls);
  port1.close();
  port2.close();
});
port2.postMessage("probe");
