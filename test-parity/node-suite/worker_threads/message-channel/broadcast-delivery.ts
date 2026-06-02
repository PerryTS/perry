import {
  BroadcastChannel,
  receiveMessageOnPort,
} from "node:worker_threads";

const sender = new BroadcastChannel("perry-broadcast");
const listener = new BroadcastChannel("perry-broadcast");
const syncReceiver = new BroadcastChannel("perry-broadcast");

let delivered = 0;
const finish = () => {
  delivered += 1;
  if (delivered !== 2) {
    return;
  }
  const afterEvent = receiveMessageOnPort(syncReceiver);
  console.log("broadcast after event:", afterEvent ? afterEvent.message : afterEvent);
  setTimeout(() => {
    sender.close();
    listener.close();
    syncReceiver.close();
  }, 25);
};

listener.onmessage = (event: any) => {
  console.log("broadcast handler:", event.type, event.data, event.target === listener);
  finish();
};
listener.addEventListener("message", (event: any) => {
  console.log("broadcast event:", event.type, event.data, event.target === listener);
  finish();
});

sender.postMessage("bc-1");

const received = receiveMessageOnPort(syncReceiver);
console.log("broadcast receive:", received ? received.message : received);
