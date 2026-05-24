import { Readable, Writable, Duplex, Transform } from "node:stream";
// destroyed flag should be false on freshly-constructed streams.
const r = new Readable({ read() {} });
const w = new Writable({ write(_c, _e, cb) { cb(); } });
const d = new Duplex({ read() {}, write(_c, _e, cb) { cb(); } });
const t = new Transform({ transform(c, _e, cb) { cb(null, c); } });
console.log("R:", r.destroyed);
console.log("W:", w.destroyed);
console.log("D:", d.destroyed);
console.log("T:", t.destroyed);
