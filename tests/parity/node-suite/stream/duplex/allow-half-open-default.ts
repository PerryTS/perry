import { Duplex } from "node:stream";
// allowHalfOpen defaults to false on Duplex — ending the writable also ends
// the readable side.
const d = new Duplex({ read() {}, write(_c, _e, cb) { cb(); } });
console.log("default allowHalfOpen:", d.allowHalfOpen);
