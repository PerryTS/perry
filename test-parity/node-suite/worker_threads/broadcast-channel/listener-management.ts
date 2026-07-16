import { BroadcastChannel } from "node:worker_threads";

const sender = new BroadcastChannel("listener-management");
const receiver = new BroadcastChannel("listener-management");
let calls = 0;

function listener(event: MessageEvent) {
  calls += 1;
  console.log("listener:", calls, event.data);
  sender.close();
  receiver.close();
}

function removed() {
  console.log("removed listener fired");
}

receiver.addEventListener("message", listener);
receiver.addEventListener("message", listener);
receiver.addEventListener("message", removed);
receiver.removeEventListener("message", removed);
sender.postMessage("once");
