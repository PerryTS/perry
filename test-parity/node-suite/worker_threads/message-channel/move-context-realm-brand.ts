import * as vm from "node:vm";
import {
  MessageChannel,
  MessagePort,
  moveMessagePortToContext,
} from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const moved = moveMessagePortToContext(port1, vm.createContext({}));

console.log(
  "realm brand:",
  moved instanceof MessagePort,
  moved.constructor.name,
);

moved.close();
port2.close();
