import { Readable } from "node:stream";
// Calling destroy() twice is idempotent — only one 'close' event fires.
let closeCount = 0;
const r = new Readable({ read() {} });
r.on("close", () => closeCount++);
r.destroy();
r.destroy();
setImmediate(() => {
  console.log("close count:", closeCount);
  console.log("destroyed:", r.destroyed);
});
