import { Duplex, finished } from "node:stream";
// finished(stream, { readable: false }, cb) only watches the writable side.
const d = new Duplex({
  read() {},
  write(_c, _e, cb) { cb(); },
});
let fired = false;
finished(d, { readable: false }, () => (fired = true));
d.end();
setImmediate(() => console.log("fired on writable end:", fired));
