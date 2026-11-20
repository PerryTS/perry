import { EventEmitter, on } from "node:events";

const ee = new EventEmitter();
const iter = on(ee, "data", { highWaterMark: 2, lowWaterMark: 1 } as any);
ee.emit("data", "a");
ee.emit("data", "b");
ee.emit("end");
let count = 0;
for await (const args of iter) {
  console.log("item:", args.join(","));
  if (++count === 2) break;
}
