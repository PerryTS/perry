import {
  BroadcastChannel,
  receiveMessageOnPort,
} from "node:worker_threads";

function receive(channel: BroadcastChannel): string {
  const packet = receiveMessageOnPort(channel);
  return packet ? JSON.stringify(packet.message) : "empty";
}

const sender = new BroadcastChannel("fanout");
const listenerA = new BroadcastChannel("fanout");
const listenerB = new BroadcastChannel("fanout");
const unrelated = new BroadcastChannel("other");

sender.postMessage({ index: 1 });
sender.postMessage({ index: 2 });

console.log("listener a:", receive(listenerA), receive(listenerA));
console.log("listener b:", receive(listenerB), receive(listenerB));
console.log("sender excluded:", receive(sender));
console.log("name isolated:", receive(unrelated));

sender.close();
listenerA.close();
listenerB.close();
unrelated.close();
