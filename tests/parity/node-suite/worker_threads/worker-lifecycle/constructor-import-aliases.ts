import { MessageChannel as BroadcastChannel, BroadcastChannel as MessageChannel, Worker as Thread } from "node:worker_threads";
import * as native from "node:worker_threads";

process.chdir("tests/parity/node-suite/worker_threads/worker-lifecycle");

const globalChannel = new globalThis.MessageChannel();
console.log("global channel:", globalChannel instanceof native.MessageChannel);
globalChannel.port1.close();
globalChannel.port2.close();
const globalBroadcast = new globalThis.BroadcastChannel("global-name");
console.log("global broadcast:", globalBroadcast.name);
globalBroadcast.close();

const channel = new BroadcastChannel();
console.log("channel:", channel instanceof native.MessageChannel, channel.port1 instanceof native.MessagePort);
const broadcast = new MessageChannel("alias-name");
console.log("broadcast:", broadcast instanceof native.BroadcastChannel, broadcast.name);
broadcast.close();

await new Promise<void>((resolve) => {
  channel.port2.on("message", (message) => {
    console.log("channel message:", message);
    channel.port1.close();
    channel.port2.close();
    resolve();
  });
  channel.port1.postMessage("alias-delivery");
});

const worker = new Thread("./natural-exit-worker.cjs");
worker.on("message", (message) => console.log("worker message:", message));
worker.on("exit", (code) => console.log("worker exit:", code));
