import { Readable, PassThrough, pipeline } from "node:stream";
// pipeline invokes its callback with the Error when a stage emits 'error'.
const r = new Readable({ read() { this.emit("error", new Error("boom")); } });
const sink = new PassThrough();
pipeline(r, sink, (err) => {
  console.log("err message:", err && err.message);
});
