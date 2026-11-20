// CommonJS forms of an EventEmitter subclass: `require("events")` as the base
// (the module IS EventEmitter), `require("events").EventEmitter`,
// util.inherits with EventEmitter.call(this), a setPrototypeOf-linked
// function constructor, and EventEmitter.call(this) alone (which, as in node,
// gives the object emitter state but no methods). No instance owns a method.
const EE = require("events");
const { EventEmitter } = require("events");
const util = require("util");
const own = (o: any, k: string) => Object.prototype.hasOwnProperty.call(o, k);
class D extends EE {}
class F extends EE.EventEmitter {}
class G extends EventEmitter {}
for (const [n, K] of [["D", D], ["F", F], ["G", G]] as any[]) {
  const o = new K();
  console.log(n, typeof o.on, typeof o.emit, typeof o.once, JSON.stringify(Object.keys(o)), own(o, "on"), own(o, "emit"));
  o.on("x", (v: any) => console.log(n, "got", v));
  console.log(n, o.emit("x", 1), o.listenerCount("x"), o.on === EE.prototype.on);
}
function L(this: any) { EE.call(this); }
util.inherits(L, EE);
const l: any = new (L as any)();
l.once("y", (v: any) => console.log("L got", v));
l.emit("y", 2);
l.emit("y", 3);
console.log("L", JSON.stringify(Object.keys(l)), own(l, "once"), l.listenerCount("y"));
function M(this: any) { EventEmitter.call(this); }
Object.setPrototypeOf(M.prototype, EventEmitter.prototype);
const m: any = new (M as any)();
m.on("z", () => console.log("M z"));
m.emit("z");
console.log("M", JSON.stringify(Object.keys(m)), JSON.stringify(Object.keys(m._events)));
function N(this: any) { EventEmitter.call(this); }
const nn: any = new (N as any)();
console.log("N", JSON.stringify(Object.keys(nn)), typeof nn.on);
