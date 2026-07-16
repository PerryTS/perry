import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

function receive(value: any): string {
  try {
    const packet = receiveMessageOnPort(value);
    return packet ? `message:${JSON.stringify(packet.message)}` : "empty";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const { port1, port2 } = new MessageChannel();
console.log("initial:", receive(port2));
port1.postMessage({ index: 1 });
port1.postMessage({ index: 2 });
console.log("fifo one:", receive(port2));
console.log("fifo two:", receive(port2));
console.log("drained:", receive(port2));

let listenerCalls = 0;
port2.on("message", () => {
  listenerCalls += 1;
});
port1.postMessage("sync-wins");
console.log("sync with listener:", receive(port2), listenerCalls);

for (const value of [null, 0, {}, []]) {
  console.log("invalid:", receive(value));
}

port1.close();
port2.close();
