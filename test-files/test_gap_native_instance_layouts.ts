// Own keys of native emitter and stream instances match node: order,
// hasOwn, Object.keys, for-in and JSON, at birth and after optional members
// (hooks, `_eventsCount`) are filled, for each class and a user subclass.
import { EventEmitter } from "node:events";
import { Stream, Readable, Writable, Duplex, Transform, PassThrough } from "node:stream";

const noop = function () {};
function hooks(): any {
  return {
    read: noop,
    write: function (_c: any, _e: any, cb: any) { cb(); },
    writev: noop,
    destroy: noop,
    final: noop,
    construct: function (cb: any) { cb(); },
    transform: function (c: any, _e: any, cb: any) { cb(null, c); },
    flush: noop,
  };
}
const probe = ["_events", "_eventsCount", "_maxListeners", "_readableState", "_writableState",
  "allowHalfOpen", "_read", "_write", "_writev", "_destroy", "_final", "_construct",
  "_transform", "_flush"];
function show(label: string, o: any) {
  const forIn: string[] = [];
  for (const k in o) if (Object.hasOwn(o, k)) forIn.push(k);
  const own = probe.filter((k) => Object.hasOwn(o, k));
  let json: string;
  try {
    json = JSON.stringify(o, (k, v) => (k === "" || typeof v !== "object" || v === null) ? v : "[o]");
  } catch (e) {
    json = "throws";
  }
  console.log(label, "names", Object.getOwnPropertyNames(o).join(","));
  console.log(label, "keys", Object.keys(o).join(","));
  console.log(label, "forin", forIn.join(","));
  console.log(label, "hasOwn", own.join(","));
  console.log(label, "json", json);
  console.log(label, "count", typeof o._eventsCount, String(o._eventsCount));
}
const classes: [string, any][] = [
  ["EventEmitter", EventEmitter], ["Stream", Stream], ["Readable", Readable],
  ["Writable", Writable], ["Duplex", Duplex], ["Transform", Transform], ["PassThrough", PassThrough],
];
for (const [name, C] of classes) {
  const a: any = new C();
  show(name + " birth", a);
  a.on("x", noop);
  show(name + " on", a);
  a.zz = 1;
  show(name + " expando", a);
  const e: any = new C();
  e.zz = 1;
  e.on("x", noop);
  show(name + " expando-then-on", e);
  show(name + " hooks", new C(hooks()));
  const d: any = new C();
  d._read = noop;
  d._write = noop;
  show(name + " hooks-after", d);
  const m: any = new C();
  m.setMaxListeners(5);
  show(name + " setmax", m);
}
class SubE extends EventEmitter { mine = 1; }
class SubR extends Readable { constructor(o?: any) { super(o); (this as any).mine = 1; } }
class SubW extends Writable { constructor(o?: any) { super(o); (this as any).mine = 1; } }
class SubD extends Duplex { constructor(o?: any) { super(o); (this as any).mine = 1; } }
class SubT extends Transform { constructor(o?: any) { super(o); (this as any).mine = 1; } }
for (const [name, C] of [["SubE", SubE], ["SubR", SubR], ["SubW", SubW], ["SubD", SubD], ["SubT", SubT]] as [string, any][]) {
  const s: any = new C();
  show(name + " birth", s);
  s.on("x", noop);
  show(name + " on", s);
  if (name !== "SubE") show(name + " hooks", new C(hooks()));
}
// A setter on a subclass prototype runs for the constructor's store (no own key).
const log: string[] = [];
class Q extends Readable {}
Object.defineProperty(Q.prototype, "_read", {
  set(v: any) { log.push("set " + typeof v); },
  get() { return noop; },
  configurable: true,
});
show("setter-read", new Q({ read: noop }));
console.log("setter log", log.join(","));
class P extends EventEmitter {}
Object.defineProperty(P.prototype, "_maxListeners", {
  set(v: any) { log.push("setmax " + String(v)); },
  get() { return 7; },
  configurable: true,
});
const p: any = new P();
show("setter-max", p);
console.log("setter log", log.join(","), p.getMaxListeners());
// The same store site filling a hole on several instances.
for (let i = 0; i < 3; i++) {
  const r: any = new Readable();
  r._read = noop;
  show("site-" + i, r);
}
// Other store forms of a hole fill.
const viaAssign: any = Object.assign(new Readable(), { _read: noop, extra: 1 });
show("assign", viaAssign);
const viaKey: any = new Writable();
const hookKey = "_final";
viaKey[hookKey] = noop;
show("computed", viaKey);
const viaReflect: any = new Duplex();
console.log("reflect", Reflect.set(viaReflect, "_write", noop));
show("reflect", viaReflect);
const sealed: any = new Readable();
Object.preventExtensions(sealed);
try {
  sealed._read = noop;
  console.log("non-extensible stored", Object.hasOwn(sealed, "_read"));
} catch (e) {
  console.log("non-extensible throws", (e as any).constructor.name);
}
// An inherited setter defined after birth runs for a hole fill.
const late: any = new Readable();
Object.defineProperty(Readable.prototype, "_destroy", {
  set(v: any) { log.push("late " + typeof v); },
  get() { return noop; },
  configurable: true,
});
late._destroy = noop;
show("late-setter", late);
console.log("setter log", log.join(","));
show("after-proto-write", new Readable());
