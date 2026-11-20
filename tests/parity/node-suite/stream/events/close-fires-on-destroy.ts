import { Readable } from "node:stream";
// 'close' fires after destroy(); fires exactly once.
let count = 0;
const r = new Readable({ read() {} });
r.on("close", () => count++);
r.destroy();
setImmediate(() => {
  setImmediate(() => console.log("close fires:", count));
});
