import { Readable } from "node:stream";
// Stream constructor with captureRejections — async handler errors caught.
const r = new Readable({ read() {}, captureRejections: true } as any);
let caught: any = null;
r.on("error", (e) => (caught = e && (e as Error).message));
r.on("data", async () => {
  throw new Error("rejection-in-data");
});
r.push("x");
r.push(null);
setImmediate(() => {
  setImmediate(() => {
    console.log("captured:", caught);
  });
});
