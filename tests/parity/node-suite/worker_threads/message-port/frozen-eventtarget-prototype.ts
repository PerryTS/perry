import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

Object.freeze(EventTarget.prototype);
console.log("frozen:", Object.isFrozen(EventTarget.prototype));

const { port1, port2 } = new MessageChannel();
try {
  const listener = () => console.log("unexpected async listener");
  port2.on("message", listener);
  console.log("registered:", port2.listenerCount("message"));
  port1.postMessage("value");
  console.log("received:", receiveMessageOnPort(port2)?.message);
  port2.off("message", listener);
  console.log("removed:", port2.listenerCount("message"));
} catch (error) {
  console.log("error:", (error as any)?.name, (error as any)?.code ?? "");
} finally {
  port1.close();
  port2.close();
}
