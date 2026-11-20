import { Transform } from "node:stream";
// Transform can have distinct readable/writable highWaterMarks via the
// readableHighWaterMark and writableHighWaterMark options.
const t = new Transform({
  readableHighWaterMark: 3,
  writableHighWaterMark: 5,
  transform(c, _e, cb) { cb(null, c); },
});
console.log("read hwm:", t.readableHighWaterMark);
console.log("write hwm:", t.writableHighWaterMark);
console.log("different:", t.readableHighWaterMark !== t.writableHighWaterMark);
