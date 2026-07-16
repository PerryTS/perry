import { BroadcastChannel } from "node:worker_threads";

const sender = new BroadcastChannel("onmessage-replacement");
const receiver = new BroadcastChannel("onmessage-replacement");
const events: string[] = [];

receiver.onmessage = (event) => events.push(`first:${event.data}`);
receiver.onmessage = (event) => {
  events.push(`second:${event.data}`);
  receiver.onmessage = null;
  sender.postMessage("after-null");
};
receiver.addEventListener("message", (event) => {
  events.push(`observer:${event.data}`);
  if (event.data === "after-null") {
    console.log("events:", events.join(","));
    console.log("handler:", receiver.onmessage);
    sender.close();
    receiver.close();
  }
});

sender.postMessage("value");
