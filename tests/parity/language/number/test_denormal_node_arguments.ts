// A Number whose IEEE bits look like a heap address (a denormal double), handed
// to the node:events / node:stream / util / TextDecoder entries. Their value
// decoder (`raw_ptr_from_value` and its copies) used to return any word below
// 2^48 as an address, so the listener / stream / buffer checks dereferenced
// the Number (SIGSEGV). A Number is a primitive there: each of these must match
// Node (ERR_INVALID_ARG_TYPE, or the ordinary result).

import { EventEmitter } from "node:events";
import * as stream from "node:stream";
import * as util from "node:util";

function bitsDouble(hi: number, lo: number): number {
  const f = new Float64Array(1);
  const u = new Uint32Array(f.buffer);
  u[0] = lo;
  u[1] = hi;
  return f[0];
}

const VALUES: [string, any][] = [
  ["min", Number.MIN_VALUE],
  ["2^21", Number.MIN_VALUE * Math.pow(2, 21)], // first word above the handle band
  ["2^28", Number.MIN_VALUE * Math.pow(2, 28)], // low heap window
  ["2^40", Number.MIN_VALUE * Math.pow(2, 40)],
  ["odd", bitsDouble(0, 0x10000003)], // unaligned
  ["mmap", bitsDouble(0x7f12, 0x34567890)], // mmap-band address shape
  ["brk", bitsDouble(0x5555, 0x55560010)], // brk-band address shape
  ["maxden", bitsDouble(0x000fffff, 0xffffffff)],
  ["-2^28", -(Number.MIN_VALUE * Math.pow(2, 28))],
  ["+0", 0],
  ["1.5", 1.5], // control: an ordinary Number
];

function id(x: any): any {
  return x;
}

const OPS: [string, (e: any) => any][] = [
  ["on", (e) => {
    const m = new EventEmitter();
    m.on("x", e);
    return m.listenerCount("x");
  }],
  ["once", (e) => {
    const m = new EventEmitter();
    m.once("x", e);
    return m.listenerCount("x");
  }],
  ["addListener", (e) => {
    const m = new EventEmitter();
    m.addListener("x", e);
    return m.listenerCount("x");
  }],
  ["prependListener", (e) => {
    const m = new EventEmitter();
    m.prependListener("x", e);
    return m.listenerCount("x");
  }],
  ["off", (e) => {
    const m = new EventEmitter();
    m.on("x", id);
    m.off("x", e);
    m.removeListener("x", e);
    return m.listenerCount("x");
  }],
  ["emit arg", (e) => {
    const m = new EventEmitter();
    let got: any;
    m.on("x", (v: any) => {
      got = v;
    });
    m.emit("x", e);
    return Object.is(got, e);
  }],
  ["readable on data", (e) => {
    const r = new stream.Readable({ read() {} });
    r.on("data", e);
    return 1;
  }],
  ["object-mode write", (e) => {
    let got: any;
    const w = new stream.Writable({
      objectMode: true,
      write(c: any, _n: any, cb: any) {
        got = c;
        cb();
      },
    });
    w.write(e);
    return Object.is(got, e);
  }],
  ["pipeline", (e) => typeof stream.pipeline(e, id)],
  ["pipeline cb", (e) => {
    const r = new stream.Readable({
      read() {
        this.push(null);
      },
    });
    const w = new stream.Writable({
      write(_c: any, _n: any, cb: any) {
        cb();
      },
    });
    return typeof stream.pipeline(r, w, e);
  }],
  ["util.diff", (e) => (util as any).diff(e, "a")],
  ["util.diff 2", (e) => (util as any).diff(["a"], e)],
  ["TextDecoder.decode", (e) => new TextDecoder().decode(e)],
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
