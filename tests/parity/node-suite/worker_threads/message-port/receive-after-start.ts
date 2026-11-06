import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
console.log("empty:", receiveMessageOnPort(port1));
port1.start();
port2.postMessage({ sequence: 1 });
port2.postMessage({ sequence: 2 });

const first = receiveMessageOnPort(port1)?.message;
const second = receiveMessageOnPort(port1)?.message;
console.log("received:", first?.sequence, second?.sequence);

port1.on("message", (value) => {
  console.log("event:", value);
  port1.close();
  port2.close();
});
port2.postMessage("barrier");
