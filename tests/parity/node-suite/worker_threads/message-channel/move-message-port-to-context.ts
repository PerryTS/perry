import * as vm from "node:vm";
import {
  MessageChannel,
  MessagePort,
  moveMessagePortToContext,
  receiveMessageOnPort,
} from "node:worker_threads";

const channel = new MessageChannel();
const port1 = channel.port1;
const port2 = channel.port2;
const moved = moveMessagePortToContext(port1, vm.createContext({ MessagePort }));

console.log("moved identity:", moved !== port1);
console.log("moved type:", typeof moved, moved.constructor.name);
console.log(
  "moved methods:",
  typeof moved.postMessage,
  typeof moved.start,
  typeof moved.close,
  typeof moved.ref,
  typeof moved.unref,
  typeof moved.hasRef,
);
console.log("moved hasRef:", moved.hasRef());

port2.postMessage("from-peer");
const inbound = receiveMessageOnPort(moved);
console.log("receive moved:", inbound ? inbound.message : inbound);

moved.postMessage("from-moved");
const outbound = receiveMessageOnPort(port2);
console.log("receive peer:", outbound ? outbound.message : outbound);

moved.close();
port2.close();
