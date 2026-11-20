import { BroadcastChannel, MessageChannel, MessagePort, receiveMessageOnPort } from "node:worker_threads";

function receive(label: string, value: any): void {
  try {
    const packet = receiveMessageOnPort(value);
    console.log(label, packet === undefined ? "empty" : JSON.stringify(packet.message));
  } catch (error: any) {
    console.log(label, error.name, error.code, error.message);
  }
}

const { port1, port2 } = new MessageChannel();
let getterReads = 0;
const fakeChannel = Object.create(BroadcastChannel.prototype);
Object.defineProperty(fakeChannel, "__perryBroadcastChannelId", {
  get() {
    getterReads++;
    return 1;
  },
});
for (const value of [undefined, null, 0, {}, [], Object.create(MessagePort.prototype), new Proxy(port2, {}), fakeChannel]) {
  receive("invalid", value);
}
console.log("getter reads", getterReads);
receive("port empty", port2);
port1.close();
port2.close();
receive("port closed", port2);

const sender = new BroadcastChannel("receive-admission");
const receiver = new BroadcastChannel("receive-admission");
let listenerCalls = 0;
receiver.onmessage = () => listenerCalls++;
receive("channel empty", receiver);
sender.postMessage("first");
sender.postMessage("second");
receive("channel first", receiver);
receive("channel second", receiver);
receive("channel drained", receiver);
sender.close();
receiver.close();
receive("channel closed", receiver);
setImmediate(() => console.log("listeners", listenerCalls));
