import { Readable } from "node:stream";
// EventEmitter captureRejections — when an async listener throws, the
// 'error' event fires (if captureRejections is set).
const r = new Readable({ read() {}, captureRejections: true } as any);
let caughtErr: any = null;
r.on("error", (e) => (caughtErr = e));
r.on("custom", async () => {
  throw new Error("async-listener-fail");
});
r.emit("custom");
setImmediate(() => {
  setImmediate(() => {
    console.log("captured:", caughtErr && (caughtErr as Error).message);
  });
});
