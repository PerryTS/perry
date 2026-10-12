import { Readable, Writable } from "node:stream";
function witness() {
const r: any = new Readable({ read() {} });
r.push("abc");
console.log(r._readableState.length, r.readableLength);
r._readableState.length = 4;
console.log(r.readableLength);
let reads = 0;
Object.defineProperty(r._readableState, "length", {
  configurable: true, get() { reads++; return 7; }
});
console.log(r.readableLength, reads);
Object.defineProperty(r._readableState, "highWaterMark", {
  configurable: true, get() { reads++; return 13; }
});
console.log(r.readableHighWaterMark, reads);
const w: any = new Writable({ write(c: any, e: any, cb: any) { cb(); } });
console.log(w.writableLength, w.writableHighWaterMark);
w._writableState.length = 5;
console.log(w.writableLength);
Object.defineProperty(w._writableState, "length", {
  configurable: true, get() { reads++; return 11; }
});
console.log(w.writableLength, reads);
}
for (let run = 0; run < 2; run++) witness();
