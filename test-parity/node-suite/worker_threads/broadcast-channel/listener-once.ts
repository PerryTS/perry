import { BroadcastChannel } from "node:worker_threads";

const sender = new BroadcastChannel("listener-once");
const receiver = new BroadcastChannel("listener-once");
const events: string[] = [];

function removed(event: MessageEvent) {
  events.push(`removed:${event.data}`);
}

receiver.addEventListener("message", removed);
receiver.removeEventListener("message", removed);
receiver.addEventListener(
  "message",
  (event) => events.push(`once:${event.data}`),
  { once: true },
);
receiver.addEventListener("message", (event) => {
  events.push(`regular:${event.data}`);
  if (event.data === "second") {
    console.log("events:", events.join(","));
    sender.close();
    receiver.close();
  }
});

sender.postMessage("first");
sender.postMessage("second");
