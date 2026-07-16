import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();

function outcome(transfer: any): string {
  try {
    port1.postMessage("value", { transfer });
    return "ok";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

console.log(
  "missing next:",
  outcome({ [Symbol.iterator]: () => ({}) }),
);
console.log(
  "number next:",
  outcome({ [Symbol.iterator]: () => ({ next: 42 }) }),
);
console.log(
  "null next:",
  outcome({ [Symbol.iterator]: () => ({ next: null }) }),
);
console.log("queue:", receiveMessageOnPort(port2));

port1.close();
port2.close();
