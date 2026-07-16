import { BroadcastChannel } from "node:worker_threads";

const sender = new BroadcastChannel("create-during-dispatch");
const first = new BroadcastChannel("create-during-dispatch");

sender.unref();
first.onmessage = (event) => {
  console.log("first:", event.data);
  first.close();

  const created = new BroadcastChannel("create-during-dispatch");
  created.onmessage = (nextEvent) => {
    console.log("created:", nextEvent.data);
    created.close();
    sender.close();
  };
  sender.postMessage("second");
};

sender.postMessage("first");
