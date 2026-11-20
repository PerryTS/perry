import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const events: string[] = [];

port1.on("message", (value) => events.push(`first:${value}`));
port1.on("message", (value) => events.push(`second:${value}`));
port1.on("message", (value) => {
  events.push(`third:${value}`);
  console.log("events:", events.join(","));
  port1.close();
  port2.close();
});

port2.postMessage("value");
