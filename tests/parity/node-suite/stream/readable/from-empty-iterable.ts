import { Readable } from "node:stream";
// Readable.from(emptyIterable) yields no data; 'end' fires immediately.
const r = Readable.from([]);
let dataCount = 0;
let endFired = false;
r.on("data", () => dataCount++);
r.on("end", () => {
  endFired = true;
  console.log("data count:", dataCount);
  console.log("end fired:", endFired);
});
