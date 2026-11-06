import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();

port1.postMessage(1, null as any);
port1.postMessage(2, undefined);
port1.postMessage(3, []);
port1.postMessage(4, {});
port1.postMessage(5, { transfer: undefined });
port1.postMessage(6, { transfer: [] });

const received: number[] = [];
for (let index = 0; index < 6; index += 1) {
  const packet = receiveMessageOnPort(port2);
  if (packet) received.push(packet.message);
}
console.log("received:", received.join(","));
console.log("drained:", receiveMessageOnPort(port2));

port1.close();
port2.close();
