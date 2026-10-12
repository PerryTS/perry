// Retain each stream side and its hooks across protected copying minors.
import { Readable, Writable, Duplex } from "node:stream";
import { EventEmitter } from "node:events";
const held: any[] = [];
let sum = 0;
for (let i = 0; i < 64; i++) {
  const o = {
    objectMode: true, highWaterMark: i + 1, autoDestroy: false,
    read(this: any) { this.push(this.label); this.push(null); },
    write(this: any, chunk: any, _encoding: any, cb: any) { sum += this.label + chunk; cb(); },
  };
  const r: any = new Readable(o);
  const w: any = new Writable(o);
  const d: any = new Duplex({ ...o, allowHalfOpen: false });
  const e: any = new EventEmitter();
  r.label = i; w.label = i; d.label = i;
  e.on("value", (n: number) => { sum += n; });
  held.push([r, w, d, e, o]);
  // Back-edge polls allocate fresh objects and move all previously held state.
  for (let j = 0; j < 4; j++) held.push({ i, j });
}
for (const item of held) {
  if (!Array.isArray(item)) continue;
  const r = item[0], w = item[1], d = item[2], e = item[3], o = item[4];
  sum += r.readableHighWaterMark + w.writableHighWaterMark + d.readableHighWaterMark + d.writableHighWaterMark;
  console.log("state", r._readableState.highWaterMark === o.highWaterMark,
    w._writableState.highWaterMark === o.highWaterMark,
    d.allowHalfOpen, JSON.stringify(Object.getOwnPropertyNames(r._readableState)));
  sum += r.read();
  w.write(1);
  e.emit("value", 2);
}
console.log("sum", sum);
