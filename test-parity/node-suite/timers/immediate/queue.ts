import { setImmediate } from "node:timers";
const order: string[] = [];
await new Promise<void>((resolve) => {
  setImmediate(() => order.push("a"));
  setImmediate(() => order.push("b"));
  setImmediate(() => {
    order.push("c");
    console.log("immediate order:", order.join(","));
    resolve();
  });
});
