import { setImmediate } from "node:timers";
// queueMicrotask must drain before setImmediate fires.
const order: string[] = [];
await new Promise<void>((resolve) => {
  setImmediate(() => order.push("immediate"));
  queueMicrotask(() => order.push("microtask"));
  setImmediate(() => {
    order.push("done");
    resolve();
  });
});
console.log("order:", order.join(","));
