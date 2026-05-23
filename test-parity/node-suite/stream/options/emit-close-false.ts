import { Readable } from "node:stream";
// emitClose: false suppresses the close event on destroy.
let fired = false;
const r = new Readable({ emitClose: false, read() {} });
r.on("close", () => (fired = true));
r.destroy();
setImmediate(() =>
  setImmediate(() => console.log("close fired (should be false):", fired))
);
