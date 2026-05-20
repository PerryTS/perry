import { setTimeout } from "node:timers";
const order: string[] = [];
setTimeout(() => order.push("timer"), 0);
order.push("sync");
await new Promise((resolve) => setTimeout(resolve, 5));
console.log("order:", order.join(","));
