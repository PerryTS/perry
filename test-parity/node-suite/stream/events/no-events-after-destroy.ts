import { Readable } from "node:stream";
// After destroy(), pushed data should NOT produce 'data' events on the
// destroyed Readable.
const r = new Readable({ read() {} });
let dataCount = 0;
r.on("data", () => dataCount++);
r.on("error", () => {});
r.destroy();
r.push("late"); // pushed after destroy — should not emit
setImmediate(() => {
  console.log("data after destroy:", dataCount);
  console.log("destroyed:", r.destroyed);
});
