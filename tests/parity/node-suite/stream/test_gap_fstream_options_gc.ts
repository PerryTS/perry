import { Readable, Writable, Duplex } from 'node:stream';
let total = 0;
const opts = {
 get highWaterMark() {
  const live: any[] = [];
  for (let i = 0; i < 100000; i++) live.push({ n: i, s: 'entry-' + i });
  total += live.length;
  return 11;
 },
 read() {}, write(_c: any, _e: any, cb: any) { cb(); },
};
const r = new Readable(opts); const w = new Writable(opts); const d = new Duplex(opts);
// Exercise collection inside the getter; the observable result is state.
console.log(total > 0, r.readableHighWaterMark, w.writableHighWaterMark, d.readableHighWaterMark, d.writableHighWaterMark);
