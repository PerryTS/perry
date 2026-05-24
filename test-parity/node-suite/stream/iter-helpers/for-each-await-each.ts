import { Readable } from "node:stream";
// forEach() with an async function — awaits each call before moving on.
const r = Readable.from([1, 2, 3]);
const events: string[] = [];
await (r as any).forEach(async (x: number) => {
  events.push("start:" + x);
  await new Promise((resolve) => setTimeout(resolve, 2));
  events.push("end:" + x);
});
console.log("order:", events.join(","));
