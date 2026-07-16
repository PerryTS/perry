import { MessageChannel } from "node:worker_threads";

const first = new MessageChannel();
const second = new MessageChannel();
const events: string[] = [];

function shared(value: any) {
  events.push(typeof value === "string" ? value : value?.type ?? "unknown");
}

first.port1.on("message", shared);
first.port1.on("close", shared);
second.port1.on("message", shared);
first.port1.off("message", shared);

second.port1.on("message", () => {
  first.port1.close();
});
first.port1.on("close", () => {
  console.log("events:", events.join(","));
  first.port2.close();
  second.port1.close();
  second.port2.close();
});

first.port2.postMessage("first-message");
second.port2.postMessage("second-message");
