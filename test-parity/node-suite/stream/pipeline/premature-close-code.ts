import { Readable, Writable, pipeline } from "node:stream";
// pipeline() fires its callback with ERR_STREAM_PREMATURE_CLOSE when a
// stream in the chain destroys before signaling end/finish.
const src = new Readable({ read() {} });
const sink = new Writable({
  write(_c, _e, cb) {
    cb();
    // Destroy mid-stream to trigger premature close
    sink.destroy();
  },
});
pipeline(src, sink, (err: any) => {
  console.log("err present:", !!err);
  console.log("err code:", err && err.code);
});
src.push("x");
