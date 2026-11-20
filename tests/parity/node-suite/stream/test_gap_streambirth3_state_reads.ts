import { Readable, Writable } from "node:stream";
let printing = true;
let sink = 0;
function log(...args: any[]) {
  const text = args.map((a) => String(a)).join(" ") + "#" + sink;
  sink = (sink + text.length) % 9973;
  if (printing) console.log(...args);
}
function witness() {
const r: any = new Readable({ read() {} });
r.push("abc");
log(r._readableState.length, r.readableLength);
r._readableState.length = 4;
log(r.readableLength);
let reads = 0;
Object.defineProperty(r._readableState, "length", {
  configurable: true, get() { reads++; return 7; }
});
log(r.readableLength, reads);
Object.defineProperty(r._readableState, "highWaterMark", {
  configurable: true, get() { reads++; return 13; }
});
log(r.readableHighWaterMark, reads);
const w: any = new Writable({ write(c: any, e: any, cb: any) { cb(); } });
log(w.writableLength, w.writableHighWaterMark);
w._writableState.length = 5;
log(w.writableLength);
Object.defineProperty(w._writableState, "length", {
  configurable: true, get() { reads++; return 11; }
});
log(w.writableLength, reads);
}
for (let run = 0; run < 52; run++) {
  printing = run < 2;
  const junk: any[] = [];
  for (let i = 0; i < 20; i++) junk.push({ i, s: "x" + i + run });
  sink = (sink + junk.length) % 9973;
  witness();
}
