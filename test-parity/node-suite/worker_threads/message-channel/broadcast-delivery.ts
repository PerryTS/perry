import {
  BroadcastChannel,
  receiveMessageOnPort,
} from "node:worker_threads";

const sender = new BroadcastChannel("perry-broadcast");
const listener = new BroadcastChannel("perry-broadcast");
const syncReceiver = new BroadcastChannel("perry-broadcast");
const globalSender = new globalThis.BroadcastChannel("perry-global-broadcast");
const globalListener = new globalThis.BroadcastChannel("perry-global-broadcast");

const observed: Record<string, string> = {};

listener.onmessage = (event: any) => {
  observed.handler = `broadcast handler: ${event.type} ${event.data} ${event.target === listener}`;
};
listener.addEventListener("message", (event: any) => {
  observed.event = `broadcast event: ${event.type} ${event.data} ${event.target === listener}`;
});
globalListener.onmessage = (event: any) => {
  observed.global = `global broadcast handler: ${event.type} ${event.data} ${event.target === globalListener}`;
};

console.log(
  "global broadcast refs:",
  globalListener.ref() === globalListener,
  globalListener.unref() === globalListener,
  typeof (globalListener as any).hasRef,
);

sender.postMessage("bc-1");
globalSender.postMessage("global-bc-1");

const received = receiveMessageOnPort(syncReceiver);
console.log("broadcast receive:", received ? received.message : received);

setImmediate(() => {
  console.log(observed.handler ?? "broadcast handler: missing");
  console.log(observed.event ?? "broadcast event: missing");
  console.log(observed.global ?? "global broadcast handler: missing");
  const afterEvent = receiveMessageOnPort(syncReceiver);
  console.log("broadcast after event:", afterEvent ? afterEvent.message : afterEvent);
  sender.close();
  listener.close();
  syncReceiver.close();
  globalSender.close();
  globalListener.close();
});
