import { Readable, PassThrough } from "node:stream";
// pipe() to a destination resumes flow on a paused source.
const r = Readable.from(["a", "b"]);
r.pause();
console.log("paused before pipe:", r.readableFlowing);
const dst = new PassThrough();
dst.on("data", () => {});
r.pipe(dst);
console.log("flowing after pipe:", r.readableFlowing);
dst.on("end", () => console.log("done"));
