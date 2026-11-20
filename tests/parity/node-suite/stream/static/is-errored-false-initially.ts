import { Readable, Writable, isErrored } from "node:stream";
// isErrored is false on a fresh, error-free stream.
const r = new Readable({ read() {} });
const w = new Writable({ write(_c, _e, cb) { cb(); } });
console.log("R:", isErrored(r));
console.log("W:", isErrored(w));
console.log("both false:", isErrored(r) === false && isErrored(w) === false);
