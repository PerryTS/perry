// #10508: `new EventEmitter()` is an ordinary object on the shared
// `EventEmitter.prototype` (the same implementation a subclass instance
// uses), not a registry handle. Its prototype identity, own state keys,
// JSON form, listener order, once / prepend wrappers, the error event,
// errorMonitor, newListener / removeListener, max-listener warnings,
// defaultMaxListeners and listenerCount(type, listener) all match node.
import { EventEmitter, errorMonitor } from "node:events";
import EE from "events";
const log = (...a: any[]) => console.log(...a);
const e = new EventEmitter();
log("proto", Object.getPrototypeOf(e) === EventEmitter.prototype, e instanceof EventEmitter, typeof e);
log("keys", JSON.stringify(Object.keys(e)), JSON.stringify(e));
log("own", Object.getOwnPropertyNames(e).join(","));
log("ctor", e.constructor === EventEmitter, e.constructor.name);
log("default import same", EE === (EventEmitter as any), new (EE as any)() instanceof EventEmitter);
const Dyn: any = EventEmitter; const d = new Dyn(); log("dyn", d instanceof EventEmitter, Object.getPrototypeOf(d) === EventEmitter.prototype);
const order: string[] = [];
e.on("newListener", (ev: any, fn: any) => order.push("new:" + String(ev)));
e.on("removeListener", (ev: any, fn: any) => order.push("rm:" + String(ev)));
const a = () => order.push("a"); const b = () => order.push("b"); const c = () => order.push("c");
e.on("x", a); e.prependListener("x", b); e.once("x", c); e.prependOnceListener("x", () => order.push("p1"));
log("count", e.listenerCount("x"), e.listeners("x").length, e.rawListeners("x").length);
log("emit1", e.emit("x"), "emit2", e.emit("x"), "none", e.emit("nobody"));
log(order.join(" "));
log("names", e.eventNames().map(String).join(","));
e.off("x", a); e.removeListener("x", b);
log("after off", e.listenerCount("x"), e.eventNames().map(String).join(","));
const s = Symbol("sym"); e.on(s, (v: any) => log("sym got", v)); e.emit(s, 7);
log("names2", e.eventNames().map(String).join(","));
// args
e.on("args", (...xs: any[]) => log("args", xs.length, xs.join("|"))); e.emit("args", 1, "two", 3, 4, 5);
// this binding
e.on("thisv", function (this: any) { log("this ok", this === e); }); e.emit("thisv");
// error event
try { e.emit("error", new Error("boom")); } catch (err: any) { log("error thrown", err.message); }
try { e.emit("error", "str"); } catch (err: any) { log("error nonerr", err.code, err.message); }
try { e.emit("error"); } catch (err: any) { log("error undef", err.code, err.message); }
e.on(errorMonitor, (er: any) => log("monitor", er.message));
try { e.emit("error", new Error("m")); } catch (err: any) { log("after monitor thrown", err.message); }
e.on("error", (er: any) => log("handled", er.message)); log("emit err handled", e.emit("error", new Error("h")));
// raw listeners once wrapper
const o = new EventEmitter(); const f = () => log("f called");
o.once("y", f); const raw: any = o.rawListeners("y")[0];
log("raw is wrapper", raw !== f, raw.listener === f, o.listeners("y")[0] === f);
raw(); log("after raw call", o.listenerCount("y"));
// listener validation
try { (o as any).on("z", 5); } catch (err: any) { log("bad listener", err.code); }
// max listeners
log("max default", o.getMaxListeners(), EventEmitter.defaultMaxListeners);
o.setMaxListeners(1); log("max set", o.getMaxListeners());
process.on("warning", (w: any) => log("warning", w.name, w.emitter === o, w.type, w.count));
o.on("w", () => {}); o.on("w", () => {});
try { o.setMaxListeners(-1); } catch (err: any) { log("bad max", err.code); }
// removeAllListeners
const r = new EventEmitter(); r.on("removeListener", (n: any) => log("rmall saw", n)); r.on("q1", () => {}); r.on("q2", () => {});
r.removeAllListeners("q1"); log("rmall one", r.eventNames().join(","));
r.removeAllListeners(); log("rmall all", r.eventNames().join(","));
// chaining
log("chain", e.on("c1", () => {}) === e, e.once("c2", () => {}) === e, e.removeAllListeners("c1") === e, e.setMaxListeners(5) === e);
// listenerCount with listener
const lc = new EventEmitter(); const g = () => {}; lc.on("k", g); lc.on("k", () => {}); log("lc fn", lc.listenerCount("k", g));
// static listenerCount
log("static lc", (EventEmitter as any).listenerCount(lc, "k"));
// remove during emit
const de = new EventEmitter(); const h1 = () => { log("h1"); de.off("t", h2); }; const h2 = () => log("h2");
de.on("t", h1); de.on("t", h2); de.emit("t"); de.emit("t");
// inheritance via util.inherits
import * as util from "node:util";
function Legacy(this: any) { EventEmitter.call(this); this.tag = "L"; }
util.inherits(Legacy, EventEmitter);
const L = new (Legacy as any)(); L.on("ping", () => log("legacy ping", L.tag)); L.emit("ping"); log("legacy inst", L instanceof EventEmitter);
// Object.create mixin
const mix: any = Object.create(EventEmitter.prototype); EventEmitter.call(mix); mix.on("m", () => log("mixin ok")); mix.emit("m");
// class subclass sanity
class Sub extends EventEmitter { z = 1; }
const sb = new Sub(); sb.on("e", () => log("sub ok", sb.z)); sb.emit("e"); log("sub proto", Object.getPrototypeOf(Sub.prototype) === EventEmitter.prototype);
// Map key / WeakMap
const wm = new WeakMap(); wm.set(e, 1); const m = new Map(); m.set(e, 2); log("weak", wm.get(e), m.get(e), e === e, new EventEmitter() === new EventEmitter());
// _events shape
const sh = new EventEmitter(); sh.on("one", () => {}); sh.on("one", () => {}); sh.on("two", () => {});
log("_events", typeof (sh as any)._events, Object.getPrototypeOf((sh as any)._events), Array.isArray((sh as any)._events.one), typeof (sh as any)._events.two, (sh as any)._eventsCount);
log("done");
