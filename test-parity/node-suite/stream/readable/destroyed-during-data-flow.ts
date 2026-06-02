import { Readable } from "node:stream";
// destroy() inside a 'data' handler — subsequent data events stop.
let dataCount = 0;
let closed = false;
const r = Readable.from(["a", "b", "c", "d", "e"]);
r.on("data", () => {
  dataCount++;
  if (dataCount === 2) r.destroy();
});
r.on("close", () => (closed = true));
r.on("error", () => {});
r.on("end", () => {});
setImmediate(() => {
  setImmediate(() => {
    console.log("data emitted:", dataCount);
    console.log("closed:", closed);
  });
});
