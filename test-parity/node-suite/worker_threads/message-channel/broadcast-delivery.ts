import {
  BroadcastChannel,
  receiveMessageOnPort,
} from "node:worker_threads";

const sender = new BroadcastChannel("perry-broadcast");
const listener = new BroadcastChannel("perry-broadcast");
const syncReceiver = new BroadcastChannel("perry-broadcast");

listener.onmessage = (event: any) =>
  console.log("broadcast handler:", event.type, event.data, event.target === listener);
listener.addEventListener("message", (event: any) =>
  console.log("broadcast event:", event.type, event.data, event.target === listener),
);

sender.postMessage("bc-1");

const received = receiveMessageOnPort(syncReceiver);
console.log("broadcast receive:", received ? received.message : received);

setTimeout(() => {
  const afterEvent = receiveMessageOnPort(syncReceiver);
  console.log("broadcast after event:", afterEvent ? afterEvent.message : afterEvent);
  sender.close();
  listener.close();
  syncReceiver.close();
}, 0);
