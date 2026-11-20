import * as worker_threads from "node:worker_threads";
import {
  BroadcastChannel as ModuleBroadcastChannel,
  MessageChannel as ModuleMessageChannel,
  MessagePort as ModuleMessagePort,
} from "node:worker_threads";

function hasFunction(value: any, name: string): boolean {
  return typeof value[name] === "function";
}

console.log(
  "global types:",
  typeof globalThis.MessageChannel,
  typeof globalThis.MessagePort,
  typeof globalThis.BroadcastChannel,
);
console.log(
  "namespace identity:",
  worker_threads.MessageChannel === globalThis.MessageChannel,
  worker_threads.MessagePort === globalThis.MessagePort,
  worker_threads.BroadcastChannel === globalThis.BroadcastChannel,
);
console.log(
  "named identity:",
  ModuleMessageChannel === globalThis.MessageChannel,
  ModuleMessagePort === globalThis.MessagePort,
  ModuleBroadcastChannel === globalThis.BroadcastChannel,
);
console.log(
  "constructor names:",
  MessageChannel.name,
  MessagePort.name,
  BroadcastChannel.name,
);
console.log(
  "constructor lengths:",
  MessageChannel.length,
  MessagePort.length,
  BroadcastChannel.length,
);
console.log(
  "prototype constructors:",
  MessageChannel.prototype.constructor === MessageChannel,
  MessagePort.prototype.constructor === MessagePort,
  BroadcastChannel.prototype.constructor === BroadcastChannel,
);

let messageChannelCall = "did-not-throw";
try {
  (globalThis.MessageChannel as any)();
} catch (_err) {
  messageChannelCall = "threw";
}
console.log("MessageChannel call:", messageChannelCall);

let messagePortCall = "did-not-throw";
try {
  (globalThis.MessagePort as any)();
} catch (_err) {
  messagePortCall = "threw";
}
console.log("MessagePort call:", messagePortCall);

let messagePortNew = "did-not-throw";
try {
  new (globalThis.MessagePort as any)();
} catch (_err) {
  messagePortNew = "threw";
}
console.log("MessagePort new:", messagePortNew);

const ChannelCtor = ModuleMessageChannel;
const channel = new ChannelCtor();
console.log(
  "channel shape:",
  channel.constructor === MessageChannel,
  Object.getPrototypeOf(channel) === MessageChannel.prototype,
  typeof channel.port1,
  typeof channel.port2,
);
console.log(
  "port shape:",
  channel.port1.constructor === MessagePort,
  Object.getPrototypeOf(channel.port1) === MessagePort.prototype,
  channel.port2.constructor === MessagePort,
  Object.getPrototypeOf(channel.port2) === MessagePort.prototype,
);
console.log(
  "port methods:",
  hasFunction(channel.port1, "postMessage"),
  hasFunction(channel.port1, "start"),
  hasFunction(channel.port1, "close"),
  hasFunction(channel.port1, "ref"),
  hasFunction(channel.port1, "unref"),
  hasFunction(channel.port1, "hasRef"),
);
console.log(
  "port events:",
  channel.port1.onmessage,
  channel.port1.onmessageerror,
);
channel.port1.close();
channel.port2.close();

const BroadcastCtor = ModuleBroadcastChannel;
const bc = new BroadcastCtor("perry-channel");
console.log(
  "broadcast shape:",
  bc.name,
  bc.constructor === BroadcastChannel,
  Object.getPrototypeOf(bc) === BroadcastChannel.prototype,
);
console.log(
  "broadcast methods:",
  hasFunction(bc, "postMessage"),
  hasFunction(bc, "close"),
  hasFunction(bc, "ref"),
  hasFunction(bc, "unref"),
);
console.log("broadcast events:", bc.onmessage, bc.onmessageerror);
console.log(
  "broadcast refs:",
  bc.ref() === bc,
  bc.unref() === bc,
  typeof (bc as any).hasRef,
);
bc.close();
