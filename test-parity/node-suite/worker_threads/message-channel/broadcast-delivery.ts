import {
  BroadcastChannel,
  receiveMessageOnPort,
} from "node:worker_threads";

const sender = new BroadcastChannel("perry-broadcast");
const listener = new BroadcastChannel("perry-broadcast");
const syncReceiver = new BroadcastChannel("perry-broadcast");
const globalSender = new globalThis.BroadcastChannel("perry-global-broadcast");
const globalListener = new globalThis.BroadcastChannel("perry-global-broadcast");

let delivered = 0;
const observed: Record<string, string> = {};
const finish = () => {
  delivered += 1;
  if (delivered !== 3) {
    return;
  }
  console.log(observed.handler);
  console.log(observed.event);
  console.log(observed.global);
  const afterEvent = receiveMessageOnPort(syncReceiver);
  console.log("broadcast after event:", afterEvent ? afterEvent.message : afterEvent);
  setTimeout(() => {
    sender.close();
    listener.close();
    syncReceiver.close();
    globalSender.close();
    globalListener.close();
  }, 25);
};

listener.onmessage = (event: any) => {
  observed.handler = `broadcast handler: ${event.type} ${event.data} ${event.target === listener}`;
  finish();
};
listener.addEventListener("message", (event: any) => {
  observed.event = `broadcast event: ${event.type} ${event.data} ${event.target === listener}`;
  finish();
});
globalListener.onmessage = (event: any) => {
  observed.global = `global broadcast handler: ${event.type} ${event.data} ${event.target === globalListener}`;
  finish();
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
