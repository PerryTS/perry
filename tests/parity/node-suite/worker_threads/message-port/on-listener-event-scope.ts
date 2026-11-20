import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const events: string[] = [];

function shared(value?: unknown) {
  events.push(typeof value === "string" ? `message:${value}` : "close");
}

port1.on("message", shared);
port1.on("message", shared);
port1.on("close", shared);
port1.on("close", shared);

port1.on("message", () => {
  port1.off("message", shared);
  port1.close();
  port2.close();
});
port1.on("close", () => {
  console.log("events:", events.join(","));
});

port2.postMessage("value");
