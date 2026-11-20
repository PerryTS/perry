import { BroadcastChannel } from "node:worker_threads";

const sender = new BroadcastChannel("close-queued-delivery");
const closed = new BroadcastChannel("close-queued-delivery");
const barrier = new BroadcastChannel("close-queued-delivery");
let closedCalls = 0;

sender.unref();
closed.onmessage = () => {
  closedCalls += 1;
};
barrier.onmessage = (event) => {
  console.log("barrier:", event.data, "closed calls:", closedCalls);
  sender.close();
  barrier.close();
};

sender.postMessage("queued");
closed.close();
