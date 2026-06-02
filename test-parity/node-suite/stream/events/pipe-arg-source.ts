import { Readable, PassThrough } from "node:stream";
// The 'pipe' event on the destination receives the source as its argument.
const r = Readable.from(["x"]);
const dst = new PassThrough();
let receivedSource: any = null;
dst.on("pipe", (src) => (receivedSource = src));
dst.on("data", () => {});
r.pipe(dst);
setImmediate(() => {
  console.log("source received:", receivedSource === r);
});
