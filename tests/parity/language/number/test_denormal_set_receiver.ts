// A Number whose IEEE bits look like a heap address (a denormal double) as the
// TARGET of an ordinary [[Set]]: `EventEmitter.prototype.on.call(e, ...)` and
// `stream.finished(e, cb)` store `_events` on their receiver. The [[Set]] fast
// path admitted a target with clear top 16 bits as a heap object and read its
// header (SIGSEGV). A Number target is a primitive: these throw like they do for
// 1.5, as in Node. Objects and a real emitter are the positive controls.
import { EventEmitter } from "events";
import * as stream from "stream";

function bitsDouble(hi: number, lo: number): number {
  const f = new Float64Array(1);
  const u = new Uint32Array(f.buffer);
  u[0] = lo;
  u[1] = hi;
  return f[0];
}

const VALUES: [string, any][] = [
  ["min", Number.MIN_VALUE],
  ["2^21", Number.MIN_VALUE * Math.pow(2, 21)],
  ["2^28", Number.MIN_VALUE * Math.pow(2, 28)],
  ["2^40", Number.MIN_VALUE * Math.pow(2, 40)],
  ["odd", bitsDouble(0, 0x10000003)],
  ["mmap", bitsDouble(0x7f12, 0x34567890)],
  ["brk", bitsDouble(0x5555, 0x55560010)],
  ["maxden", bitsDouble(0x000fffff, 0xffffffff)],
  ["-2^28", -(Number.MIN_VALUE * Math.pow(2, 28))],
  ["+0", 0],
  ["1.5", 1.5],
  ["emitter", new EventEmitter()],
];

function f(): void {}

const OPS: [string, (e: any) => any][] = [
  ["on.call", (e) => typeof EventEmitter.prototype.on.call(e, "x", f)],
  ["addListener.call", (e) => typeof EventEmitter.prototype.addListener.call(e, "x", f)],
  // The control asks only that a real stream is accepted (Perry's return value of
  // `finished` is a separate gap).
  ["finished", (e) => {
    stream.finished(e instanceof EventEmitter ? new stream.PassThrough() : e, f);
    return "accepted";
  }],
  ["Reflect.set", (e) => Reflect.set(e, "x", 1)],
];

for (const [name, op] of OPS) {
  let line = name + ":";
  for (const [label, e] of VALUES) {
    let r: string;
    try {
      r = String(op(e));
    } catch (x: any) {
      r = "THROW " + (x && x.name);
    }
    line += " " + label + "=" + r;
  }
  console.log(line);
}
