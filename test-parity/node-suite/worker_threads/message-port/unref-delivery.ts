import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const events: string[] = [];

port1.on("message", (value) => {
  events.push(String(value));
  if (events.length === 3) {
    console.log("events:", events.join(","));
    console.log("refs:", port1.hasRef(), port2.hasRef());
    port1.close();
    port2.close();
  }
});
port1.unref();
port2.ref();

port2.postMessage("a");
port2.postMessage("b");
port2.postMessage("c");
