// EventEmitter methods live on ONE shared EventEmitter.prototype, as in node:
// an emitter instance owns only node's state (`_events`, `_eventsCount`,
// `_maxListeners`), and that state is the real listener store. Covers class
// subclasses, overrides with super calls, util.inherits / EventEmitter.call
// function constructors, Object.create(EventEmitter.prototype) and a
// setPrototypeOf mixin, plus the listener API over that store.
import { EventEmitter } from "node:events";
import * as util from "node:util";

const METHODS = [
  "on", "addListener", "once", "off", "removeListener", "emit",
  "prependListener", "prependOnceListener", "removeAllListeners",
  "listenerCount", "listeners", "rawListeners", "eventNames",
  "setMaxListeners", "getMaxListeners",
];
const own = (o: any, k: string) => Object.prototype.hasOwnProperty.call(o, k);
const ownMethods = (o: any) => METHODS.filter((m) => own(o, m)).length;
const show = (label: string, o: any) => {
  console.log(label, "keys", JSON.stringify(Object.keys(o)));
  console.log(label, "names", JSON.stringify(Object.getOwnPropertyNames(o)));
  console.log(label, "own methods", ownMethods(o), "own on", own(o, "on"));
};

// ── 1. The prototype ─────────────────────────────────────────────────
const proto: any = EventEmitter.prototype;
console.log("proto names", JSON.stringify(Object.getOwnPropertyNames(proto).sort()));
for (const k of ["on", "emit", "_events", "_eventsCount", "_maxListeners"]) {
  const d = Object.getOwnPropertyDescriptor(proto, k)!;
  console.log("desc", k, typeof d.value, d.writable, d.enumerable, d.configurable);
}
console.log("aliases", proto.addListener === proto.on, proto.off === proto.removeListener);
console.log("proto state", proto._events, proto._eventsCount, proto._maxListeners);

// ── 2. A class subclass ──────────────────────────────────────────────
class Bus extends EventEmitter {}
const b: any = new Bus();
show("sub", b);
console.log("sub inherits", METHODS.every((m) => typeof b[m] === "function"));
console.log("sub shares", b.on === proto.on, b.emit === new Bus().emit, b.on === b.addListener);
console.log("sub state", Object.getPrototypeOf(b._events) === null, b._eventsCount, b._maxListeners);
console.log("sub proto chain", Object.getPrototypeOf(Bus.prototype) === proto, b instanceof EventEmitter);

// A subclass with its own fields: the same own state plus the field.
class Fielded extends EventEmitter {
  count = 0;
  label: string;
  constructor() {
    super();
    this.label = "f";
  }
}
const fd: any = new Fielded();
console.log("fielded keys", JSON.stringify(Object.keys(fd).sort()), ownMethods(fd));

// ── 3. Overrides and super ───────────────────────────────────────────
const log: string[] = [];
class Logged extends EventEmitter {
  emit(ev: string | symbol, ...args: any[]): boolean {
    log.push("emit:" + String(ev));
    return super.emit(ev, ...args);
  }
  on(ev: string | symbol, fn: (...a: any[]) => void): this {
    log.push("on:" + String(ev));
    return super.on(ev, fn);
  }
}
class Deeper extends Logged {
  emit(ev: string | symbol, ...args: any[]): boolean {
    log.push("deeper:" + String(ev));
    return super.emit(ev, ...args);
  }
}
const lg: any = new Deeper();
console.log("override own", own(lg, "emit"), own(lg, "on"), ownMethods(lg));
lg.on("a", (x: number) => log.push("a=" + x));
lg.addListener("a", (x: number) => log.push("a2=" + x));
lg.on("newListener", (ev: string) => log.push("new:" + ev));
lg.on("b", () => log.push("b"));
console.log("emit result", lg.emit("a", 7), lg.emit("nobody"));
console.log("log", log.join(","));
console.log("override counts", lg.listenerCount("a"), JSON.stringify(lg.eventNames()));

// ── 4. util.inherits and EventEmitter.call(this) ─────────────────────
function Legacy(this: any) {
  EventEmitter.call(this);
  this.name = "legacy";
}
util.inherits(Legacy, EventEmitter);
const lgc: any = new (Legacy as any)();
show("inherits", lgc);
lgc.once("ping", (v: number) => console.log("inherits ping", v));
lgc.emit("ping", 1);
lgc.emit("ping", 2);
console.log("inherits after", lgc.listenerCount("ping"), JSON.stringify(Object.keys(lgc)));

function Bare(this: any) {
  EventEmitter.call(this);
}
const bare: any = new (Bare as any)();
console.log("call only", JSON.stringify(Object.keys(bare)), typeof bare.on);

// ── 5. Object.create(EventEmitter.prototype) and a mixin ─────────────
const oc: any = Object.create(EventEmitter.prototype);
console.log("ocreate before", JSON.stringify(Object.keys(oc)), oc._eventsCount);
oc.on("q", (v: number) => console.log("ocreate q", v));
oc.emit("q", 9);
console.log("ocreate after", JSON.stringify(Object.keys(oc)), oc._eventsCount);

const mix: any = { name: "mix" };
Object.setPrototypeOf(mix, EventEmitter.prototype);
EventEmitter.call(mix);
show("mixin", mix);
mix.once("w", (v: number) => console.log("mixin w", v));
mix.emit("w", 1);
mix.emit("w", 2);

// ── 6. The listener API over the `_events` store ─────────────────────
const e: any = new Bus();
const f1 = () => console.log("f1");
const f2 = () => console.log("f2");
const f3 = () => console.log("f3");
e.on("x", f1);
console.log("store one", typeof e._events.x, e._events.x === f1, e._eventsCount);
e.prependListener("x", f2);
e.once("x", f3);
console.log("store many", Array.isArray(e._events.x), e._events.x.length, e._eventsCount);
console.log("listeners", e.listeners("x").map((f: any) => f === f1 ? "f1" : f === f2 ? "f2" : f === f3 ? "f3" : "?").join(","));
const raw = e.rawListeners("x");
console.log("raw once wrapper", typeof raw[2], raw[2] === f3, raw[2].listener === f3);
e.emit("x");
console.log("after once", e.listenerCount("x"));
e.prependOnceListener("x", f3);
e.emit("x");
e.off("x", f1);
console.log("after off", e.listenerCount("x"), e._events.x === f2);
e.removeListener("x", f2);
console.log("after remove", e.listenerCount("x"), JSON.stringify(Object.keys(e._events)), e._eventsCount);
e.on("y", f1);
e.on("z", f2);
const sym = Symbol("s");
e.on(sym, f3);
console.log("names", e.eventNames().map(String).join(","));
e.removeAllListeners("y");
console.log("names2", e.eventNames().map(String).join(","));
e.removeAllListeners();
console.log("names3", JSON.stringify(e.eventNames()), e._eventsCount);
console.log("max", e.getMaxListeners(), own(e, "_maxListeners"));
e.setMaxListeners(3);
console.log("max2", e.getMaxListeners(), e._maxListeners);

// newListener / removeListener meta events.
const meta: any = new Bus();
const metaLog: string[] = [];
meta.on("removeListener", (ev: string) => metaLog.push("rm:" + String(ev)));
meta.on("newListener", (ev: string) => metaLog.push("new:" + String(ev)));
meta.on("k", f1);
meta.once("k", f2);
meta.emit("k");
meta.removeAllListeners("k");
console.log("meta", metaLog.join(","));

// ── 7. Errors ────────────────────────────────────────────────────────
const er: any = new Bus();
try {
  er.emit("error", new Error("boom"));
} catch (err: any) {
  console.log("threw", err.message);
}
er.on(EventEmitter.errorMonitor, (err: Error) => console.log("monitor", err.message));
try {
  er.emit("error", new Error("watched"));
} catch (err: any) {
  console.log("threw2", err.message);
}
er.on("error", (err: Error) => console.log("handled", err.message));
console.log("emit error", er.emit("error", new Error("ok")));
try {
  er.on("x", 42);
} catch (err: any) {
  console.log("bad listener", err.name, err.code);
}
console.log("rejection symbol", EventEmitter.captureRejectionSymbol === Symbol.for("nodejs.rejection"));

// ── 8. Many emitters under a moving collector ────────────────────────
let total = 0;
for (let i = 0; i < 3000; i++) {
  const em: any = i % 2 ? new Bus() : new Deeper();
  em.on("tick", (n: number) => {
    const junk = [];
    for (let j = 0; j < 20; j++) junk.push({ j, s: "x" + j });
    total += n + junk.length;
  });
  em.once("tick", (n: number) => { total += n; });
  em.emit("tick", 1);
  em.emit("tick", 2);
  if (em.listenerCount("tick") !== 1 || ownMethods(em) !== 0) console.log("BAD", i);
}
console.log("stress", total, log.length > 0);

// ── 9. Every heritage form reaches the one prototype ────────────────
import { EventEmitter as AliasedEE } from "node:events";
import eventsDefault from "node:events";
const Bound = EventEmitter;
class FormA extends AliasedEE {}
class FormB extends Bound {}
const FormC = class extends EventEmitter {};
const Mixin = <T extends new (...a: any[]) => any>(Base: T) => class extends Base { mixed = true; };
class FormD extends Mixin(EventEmitter) {}
class FormF extends eventsDefault {}
class FormG extends eventsDefault.EventEmitter {}
function pickBase(): typeof EventEmitter { return EventEmitter; }
class FormH extends pickBase() {}
for (const [n, K] of [["A", FormA], ["B", FormB], ["C", FormC], ["D", FormD], ["F", FormF], ["G", FormG], ["H", FormH]] as any) {
  const o = new K();
  let got = 0;
  o.on("x", (v: number) => { got += v; });
  o.once("x", (v: number) => { got += 10 * v; });
  const r1 = o.emit("x", 1);
  const r2 = o.emit("x", 2);
  console.log("form", n, r1, r2, got, o.listenerCount("x"), JSON.stringify(Object.keys(o).sort()), ownMethods(o), o instanceof EventEmitter);
}

// ── 10. captureRejections through super(opts) ────────────────────────
class Captures extends EventEmitter {
  constructor() { super({ captureRejections: true }); }
}
const cap: any = new Captures();
console.log("capture keys", JSON.stringify(Object.keys(cap)), ownMethods(cap));
cap.on("error", (err: Error) => console.log("captured", err.message));
cap.on("job", async (n: number) => { throw new Error("async boom " + n); });
cap.emit("job", 1);
class Hooked extends EventEmitter {
  constructor() { super({ captureRejections: true }); }
  [Symbol.for("nodejs.rejection")](err: Error, ev: string, n: number) {
    console.log("rejection hook", err.message, ev, n);
  }
}
const hooked: any = new Hooked();
hooked.on("job", async () => { throw new Error("hooked"); });
hooked.emit("job", 2);
class NotCaptured extends EventEmitter {}
const quiet: any = new NotCaptured();
quiet.on("job", async () => 1);
quiet.emit("job");
try {
  class BadOption extends EventEmitter {
    constructor() { super({ captureRejections: 1 as any }); }
  }
  new BadOption();
} catch (err: any) {
  console.log("bad option", err.name, err.code);
}
setTimeout(() => console.log("done"), 10);
