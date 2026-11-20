import { Duplex } from "node:stream";
// allowHalfOpen: true keeps the readable half alive after writable ends.
const d = new Duplex({
  allowHalfOpen: true,
  read() {},
  write(_c, _e, cb) { cb(); },
});
console.log("allowHalfOpen:", d.allowHalfOpen);
