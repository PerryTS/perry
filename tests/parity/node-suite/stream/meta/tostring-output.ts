import { Readable, Writable, Duplex, Transform, PassThrough } from "node:stream";
// String(stream) — by default a JS object stringifies as [object Object].
// Stream classes do not override toString().
const r = new Readable({ read() {} });
const w = new Writable({ write(_c, _e, cb) { cb(); } });
const d = new Duplex({ read() {}, write(_c, _e, cb) { cb(); } });
const t = new Transform({ transform(c, _e, cb) { cb(null, c); } });
const p = new PassThrough();
console.log("R:", String(r));
console.log("W:", String(w));
console.log("D:", String(d));
console.log("T:", String(t));
console.log("P:", String(p));
