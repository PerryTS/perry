import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const events: string[] = [];

port1.onmessage = (event) => events.push(`first:${event.data}`);
port1.onmessage = (event) => {
  events.push(`second:${event.data}`);
  port1.onmessage = null;
  port2.postMessage("after-null");
  setImmediate(() => {
    console.log("events:", events.join(","));
    console.log("handler:", port1.onmessage);
    port1.close();
    port2.close();
  });
};

port2.postMessage("value");
