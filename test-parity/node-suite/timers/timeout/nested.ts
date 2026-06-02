import { setTimeout } from "node:timers";
const order: string[] = [];
await new Promise<void>((resolve) => {
  setTimeout(() => {
    order.push("outer");
    setTimeout(() => {
      order.push("inner");
      console.log("nested:", order.join(","));
      resolve();
    }, 0);
  }, 0);
});
