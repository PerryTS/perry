import { setImmediate } from "node:timers";
const order: string[] = [];
await new Promise<void>((resolve) => {
  setImmediate(() => {
    order.push("outer");
    setImmediate(() => {
      order.push("inner");
      console.log("nested immediate:", order.join(","));
      resolve();
    });
  });
});
