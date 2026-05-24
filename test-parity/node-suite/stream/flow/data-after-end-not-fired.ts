import { Readable } from "node:stream";
// After 'end', further push() do nothing (no more data events).
let dataCount = 0;
const r = new Readable({ read() {} });
r.on("data", () => dataCount++);
r.push("a");
r.push(null);
r.on("end", () => {
  setImmediate(() => {
    r.push("late"); // should be ignored
    setImmediate(() => {
      console.log("data count:", dataCount);
    });
  });
});
