import * as worker_threads from "node:worker_threads";

const channel = new worker_threads.MessageChannel();
const port1 = channel.port1;
const port2 = channel.port2;

port1.on("message", (value: any) => console.log("port on:", value));
port1.addEventListener("message", (event: any) =>
  console.log("port event:", event.type, event.data, event.target === port1),
);
port1.onmessage = (event: any) =>
  console.log("port handler:", event.type, event.data);
port1.on("close", () => console.log("port close"));

port2.postMessage("async-1");
port2.postMessage("sync-2");

const received = worker_threads.receiveMessageOnPort(port1);
console.log("receive:", received ? received.message : received);

setTimeout(() => {
  port1.close();
  port2.postMessage("after-close");

  setTimeout(() => {
    const afterClose = worker_threads.receiveMessageOnPort(port1);
    console.log("after close receive:", afterClose ? afterClose.message : afterClose);
    port2.close();
  }, 0);
}, 20);
