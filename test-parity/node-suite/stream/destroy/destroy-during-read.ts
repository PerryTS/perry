import { Readable } from "node:stream";
// destroy() inside _read() — the stream cancels mid-pull.
let readCalls = 0;
const r = new Readable({
  read() {
    readCalls++;
    if (readCalls === 1) this.destroy();
  },
});
r.on("error", () => {});
r.on("close", () => {
  console.log("read calls:", readCalls);
  console.log("destroyed:", r.destroyed);
});
r.on("data", () => {});
