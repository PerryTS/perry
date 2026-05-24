import { Readable, PassThrough } from "node:stream";
// pipe() adds an internal 'unpipe' listener on dst (Node tracks via dst.listenerCount).
const r = Readable.from(["x"]);
const dst = new PassThrough();
const before = dst.listenerCount("unpipe");
r.pipe(dst);
const after = dst.listenerCount("unpipe");
console.log("before:", before);
console.log("after:", after);
console.log("incremented:", after > before);
dst.on("data", () => {});
