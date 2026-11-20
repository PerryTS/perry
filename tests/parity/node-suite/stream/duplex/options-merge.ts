import { Duplex } from "node:stream";
// Duplex accepts shared options (objectMode) and separate readable/writable
// options (readableObjectMode / writableObjectMode).
const d = new Duplex({
  readableObjectMode: true,
  writableObjectMode: false,
  read() {},
  write(_c, _e, cb) { cb(); },
});
console.log("read obj mode:", d.readableObjectMode);
console.log("write obj mode:", d.writableObjectMode);
