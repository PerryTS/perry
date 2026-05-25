import { Readable } from "node:stream";
// push('') — empty string is a valid chunk, NOT an end signal.
// Only push(null) signals end.
const r = new Readable({ read() {} });
let dataCount = 0;
let endFired = false;
r.on("data", () => dataCount++);
r.on("end", () => (endFired = true));
r.push("");
r.push("x");
r.push(null);
setImmediate(() => {
  setImmediate(() => {
    console.log("data count:", dataCount);
    console.log("end fired:", endFired);
  });
});
