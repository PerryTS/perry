import { Readable } from "node:stream";
// autoDestroy: false — after 'end' the stream is NOT auto-destroyed; 'close'
// does not fire until you destroy() explicitly.
const r = new Readable({ autoDestroy: false, read() {} });
let closed = false;
r.on("close", () => (closed = true));
r.on("data", () => {});
r.on("end", () => {
  setImmediate(() => {
    console.log("destroyed after end:", r.destroyed);
    console.log("closed:", closed);
  });
});
r.push("a");
r.push(null);
