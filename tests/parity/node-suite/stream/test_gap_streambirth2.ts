// Stream construction: observable layout, normalization and subclass state.
import { Readable, Writable, Duplex } from "node:stream";
import { EventEmitter } from "node:events";
function state(label: string, s: any) {
  console.log(label, JSON.stringify(Object.keys(s)), JSON.stringify(Object.getOwnPropertyNames(s)));
  for (const key of ["_readableState", "_writableState"]) {
    const v = s[key];
    if (v) {
      console.log(key, JSON.stringify(Object.keys(v)), JSON.stringify(Object.getOwnPropertyNames(v)));
      console.log(v.highWaterMark, v.length, v.objectMode, v.emitClose, v.autoDestroy);
    }
  }
  console.log("flags", s.destroyed, s.closed, s.readable, s.writable, s.allowHalfOpen);
}
const opts = {
  objectMode: true, highWaterMark: 7, emitClose: false, autoDestroy: false,
  read() {}, write(_c: any, _e: any, cb: any) { cb(); },
};
state("Readable-default", new Readable());
state("Writable-default", new Writable());
state("Duplex-default", new Duplex());
state("Readable-options", new Readable(opts));
state("Writable-options", new Writable(opts));
state("Duplex-options", new Duplex({ ...opts, allowHalfOpen: false }));
class R extends Readable { constructor(o: any) { super(o); } }
class W extends Writable { constructor(o: any) { super(o); } }
class D extends Duplex { constructor(o: any) { super(o); } }
state("Readable-subclass", new R(opts));
state("Writable-subclass", new W(opts));
state("Duplex-subclass", new D(opts));
const a: any = new Readable(opts);
const b: any = new Readable(opts);
console.log("independent", a._readableState !== b._readableState,
  a._readableState.buffer !== b._readableState.buffer,
  a._events !== b._events);
const e: any = new EventEmitter({ captureRejections: true });
console.log("emitter", JSON.stringify(Object.keys(e)), JSON.stringify(Object.getOwnPropertyNames(e)));
e.setMaxListeners(3); console.log("max", e.getMaxListeners());
class E extends EventEmitter { constructor(o: any) { super(o); } }
const es: any = new E({ captureRejections: false });
console.log("emitter-subclass", JSON.stringify(Object.keys(es)), JSON.stringify(Object.getOwnPropertyNames(es)), es.getMaxListeners());
const side: any = new Duplex({
  objectMode: false, readableObjectMode: true, writableObjectMode: false,
  highWaterMark: 5, readableHighWaterMark: 3, writableHighWaterMark: 9,
  decodeStrings: false, defaultEncoding: "ascii", allowHalfOpen: false,
});
console.log("sides", side.readableObjectMode, side.writableObjectMode,
  side.readableHighWaterMark, side.writableHighWaterMark,
  side._writableState.decodeStrings, side._writableState.defaultEncoding, side.allowHalfOpen);
const enc: any = new Readable({ encoding: "utf8", read() {} });
console.log("encoding", enc.readableEncoding, enc._readableState.encoding);
const hook: any = {
  read() {}, write(_c: any, _e: any, cb: any) { cb(); },
  final(cb: any) { cb(); }, destroy(err: any, cb: any) { cb(err); },
};
for (const C of [Readable, Writable, Duplex]) {
  const s: any = new C(hook);
  console.log("hooks", C.name, s._read === hook.read, s._write === hook.write,
    s._destroy === hook.destroy, s._final === hook.final,
    JSON.stringify(Object.keys(s)));
}
for (const value of [-1, 1.5, NaN, "4", null]) {
  for (const C of [Readable, Writable, Duplex]) {
    try { new C({ highWaterMark: value } as any); console.log("hwm", C.name, String(value), "ok"); }
    catch (err: any) { console.log("hwm", C.name, String(value), err.code); }
  }
}
for (const value of ["invalid-encoding", 2]) {
  try { new Readable({ encoding: value } as any); console.log("encoding-validation", String(value), "ok"); }
  catch (err: any) { console.log("encoding-validation", String(value), err.code); }
}
const from: any = Readable.from([1, 2]);
console.log("from", from.readableObjectMode, from.readableHighWaterMark,
  JSON.stringify(Object.getOwnPropertyNames(from._readableState)));
