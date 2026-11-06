import { Readable } from "node:stream";
// Readable({captureRejections: true}) — listener throws are caught.
const r = new Readable({ read() {}, captureRejections: true } as any);
let caughtErr: any = null;
r.on("error", (e) => (caughtErr = e && (e as Error).message));
r.on("custom", async () => {
  throw new Error("listener-fail");
});
r.emit("custom");
setImmediate(() => {
  setImmediate(() => {
    console.log("caught:", caughtErr);
  });
});
