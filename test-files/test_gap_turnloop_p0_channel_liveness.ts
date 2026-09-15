import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const sync = new MessageChannel();
sync.port1.postMessage("sync");
console.log(receiveMessageOnPort(sync.port2)?.message);
sync.port1.close();
sync.port2.close();

const channel = new MessageChannel();
channel.port1.postMessage("queued before handler");
channel.port2.onmessage = (event) => {
  console.log(event.data);
  channel.port2.onmessage = null;
  channel.port1.close();
  channel.port2.close();
};

const cancelled = new MessageChannel();
cancelled.port2.onmessage = () => console.log("unexpected");
cancelled.port1.postMessage("cancelled");
cancelled.port2.onmessage = null;
cancelled.port1.close();
cancelled.port2.close();
