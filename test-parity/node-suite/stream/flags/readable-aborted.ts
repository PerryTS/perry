import { Readable } from "node:stream";
// readable.readableAborted is true when destroy() was called before
// the stream ended naturally (Node 16+).
const r = new Readable({ read() {} });
console.log("readableAborted before destroy:", r.readableAborted);
r.destroy(new Error("abort"));
r.on("error", () => {});
setImmediate(() => {
  console.log("readableAborted after destroy(err):", r.readableAborted);
  console.log("typeof:", typeof r.readableAborted);
});
