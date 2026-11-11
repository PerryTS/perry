import { Readable } from "node:stream";
// emit() on a destroyed stream — listeners still fire (emit is just EE.emit).
const r = new Readable({ read() {} });
let count = 0;
r.on("error", () => {});
r.on("custom", () => count++);
r.destroy();
r.emit("custom");
setImmediate(() => {
  console.log("destroyed:", r.destroyed);
  console.log("custom listener fired:", count);
});
