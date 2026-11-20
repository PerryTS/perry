import { Readable, Writable } from "node:stream";
// When dst.write() returns false, src auto-pauses; resumes after 'drain'.
let srcEvents: string[] = [];
const src = new Readable({ highWaterMark: 1, read() {} });
src.on("pause", () => srcEvents.push("pause"));
src.on("resume", () => srcEvents.push("resume"));
const w = new Writable({
  highWaterMark: 1,
  write(_c, _e, cb) { setImmediate(cb); },
});
src.pipe(w);
src.push("aa");
src.push("bb");
src.push(null);
w.on("finish", () => console.log("src events:", srcEvents.join(",")));
