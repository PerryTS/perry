import { setTimeout } from "node:timers";
const order: string[] = [];
await new Promise<void>((resolve) => {
  setTimeout(() => order.push("a"), 0);
  setTimeout(() => order.push("b"), 0);
  setTimeout(() => {
    order.push("c");
    console.log("timer order:", order.join(","));
    resolve();
  }, 0);
});
