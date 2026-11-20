import { Readable, Writable, Duplex, Transform, PassThrough, setDefaultHighWaterMark } from "node:stream";
function flags(s: any) {
 console.log(s.readableHighWaterMark, s.writableHighWaterMark, s.readableObjectMode, s.writableObjectMode,
  s.readableEncoding, s.allowHalfOpen, s.destroyed, s.closed);
 if (s._readableState) console.log("r", s._readableState.highWaterMark, s._readableState.objectMode, s._readableState.emitClose, s._readableState.autoDestroy, s._readableState.encoding);
 if (s._writableState) console.log("w", s._writableState.highWaterMark, s._writableState.objectMode, s._writableState.emitClose, s._writableState.autoDestroy, s._writableState.decodeStrings, s._writableState.defaultEncoding);
}
for (const C of [Readable, Writable, Duplex, Transform, PassThrough]) {
 flags(new C());
 flags(new C({ objectMode: true, highWaterMark: 0, decodeStrings: false, emitClose: false, autoDestroy: false, allowHalfOpen: false }));
 flags(new C({ highWaterMark: 5, readableHighWaterMark: 3, writableHighWaterMark: 7, readableObjectMode: true, writableObjectMode: true }));
 for (const v of [-1, 1.5, NaN, Infinity, "4", null, undefined, 0, 3]) {
  try { const s: any = new C({ highWaterMark: v } as any); console.log("hwm", C.name, String(v), s.readableHighWaterMark, s.writableHighWaterMark); }
  catch (e: any) { console.log("hwm", C.name, String(v), e.name, e.code); }
 }
 for (const v of ["bad-encoding", 2, false, "", "ascii", "utf8", "utf-8"]) {
  try { new C({ defaultEncoding: v } as any); console.log("defaultEncoding", C.name, String(v), "ok"); }
  catch (e: any) { console.log("defaultEncoding", C.name, String(v), e.name, e.code); }
 }
}
const a: any = new Readable({ read() {} });
a._readableState.highWaterMark = 9;
a.push("abc");
console.log("live", a.readableHighWaterMark, a.readableLength, a._readableState.length, a.read().toString(), a._readableState.length);
const f: any = Readable.from([1,2]);
console.log("from", f.readableObjectMode, f.readableHighWaterMark, Object.getOwnPropertyNames(f).join(","), Object.getOwnPropertyNames(f._readableState).join(","));

for (const C of [Duplex, Transform, PassThrough]) {
 const s: any = new C({ readable: false, writable: false });
 console.log("disabled", C.name, s.readable, s.readableEnded, s.writable, s.writableEnded, s.writableFinished);
 console.log(Object.keys(s._readableState).join(","), Object.getOwnPropertyNames(s._readableState).join(","), s._readableState.readable);
 console.log(Object.keys(s._writableState).join(","), Object.getOwnPropertyNames(s._writableState).join(","), s._writableState.writable);
}
for (const C of [Readable, Writable, Duplex]) for (const signal of [null, false, 0, "", undefined]) {
 const s: any = new C({ signal } as any);
 console.log("signal-falsy", C.name, String(signal), s.destroyed);
}
