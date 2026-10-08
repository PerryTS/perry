function bindAtSite(f: any, receiver: any, arg: number): any { return f.bind(receiver, arg); }
function callAtSite(f: any, receiver: any, arg: number): any { return f.call(receiver, arg); }
function applyAtSite(f: any, receiver: any, arg: number): any { return f.apply(receiver, [arg]); }
function add(this: any, a: number, b: number) { return this.bias + a + b; }
const arrow = (a: number, b: number) => a + b;
for (let i = 0; i < 6; i++) {
  const b = bindAtSite(add, { bias: 10 }, i);
  console.log("function", b(2), b.name, b.length);
  const a = bindAtSite(arrow, null, i);
  console.log("arrow", a(2), a.name, a.length);
  console.log("call/apply", callAtSite(arrow, null, i), applyAtSite(arrow, null, i));
}
const first = bindAtSite(add, { bias: 4 }, 1);
const second = bindAtSite(first, { bias: 100 }, 2);
console.log("bound", second(), second.name, second.length);
class Widget { n: number; constructor(a: number, b: number) { this.n = a + b; } }
const BoundWidget: any = bindAtSite(Widget, null, 7);
const w = new BoundWidget(3);
console.log("class", w.n, w instanceof Widget, w instanceof BoundWidget, BoundWidget.name, BoundWidget.length);
function Ctor(this: any, a: number, b: number) { this.n = a + b; }
const BoundCtor: any = bindAtSite(Ctor, { n: 99 }, 5);
const c = new BoundCtor(6);
console.log("new", c.n, c instanceof Ctor, c instanceof BoundCtor);
Object.defineProperty(add, "name", { value: "renamed", configurable: true });
Object.defineProperty(add, "length", { value: 7, configurable: true });
const renamed = bindAtSite(add, { bias: 1 }, 2);
console.log("metadata", renamed.name, renamed.length, renamed(3));
const reads: string[] = [];
Object.defineProperty(add, "length", { get() {
  reads.push("length");
  Object.defineProperty(add, "name", { value: "changedByLength", configurable: true });
  return 9;
}, configurable: true });
const getterBound = bindAtSite(add, { bias: 0 }, 1);
console.log("getter order", reads.join(","), getterBound.name, getterBound.length);
Object.defineProperty(add, "name", { get() { reads.push("name"); return "getterName"; }, configurable: true });
const both = bindAtSite(add, { bias: 0 }, 1);
console.log("both", reads.join(","), both.name, both.length);
function getterTarget(a: number, b: number) { return a + b; }
const order: string[] = [];
Object.defineProperty(getterTarget, "length", { get() { order.push("length"); return 3; }, configurable: true });
Object.defineProperty(getterTarget, "name", { get() { order.push("name"); return "visible"; }, configurable: true });
const observed = bindAtSite(getterTarget, null, 1);
console.log("observable order", order.join(","), observed.name, observed.length);
function replaceProperty(o: any, k: string, v: any) { o[k] = v; }
const saved = Function.prototype.bind;
replaceProperty(Function.prototype, "bind", function(this: any, receiver: any, arg: any) {
  return () => "patched:" + arg;
} as any);
console.log("prototype patch", bindAtSite(arrow, null, 12)());
replaceProperty(Function.prototype, "bind", saved);
(arrow as any).bind = function(receiver: any, arg: any) { return () => "own:" + arg; };
console.log("own patch", bindAtSite(arrow, null, 13)());
replaceProperty(arrow, "bind", saved);
for (let i = 0; i < 6; i++) {
  console.log("own native", bindAtSite(arrow, null, i)(2));
}
try { Reflect.apply(saved, 123, [null]); }
catch (e: any) { console.log("bind brand", e.name); }
function untouched(a: number) { return a + 1; }
function replaceDuringArg() {
  (untouched as any).bind = () => () => "wrong";
  return null;
}
const beforeArgs = (untouched as any).bind(replaceDuringArg(), 4);
console.log("read before args", beforeArgs());
const oldProtoName = Object.getOwnPropertyDescriptor(Function.prototype, "name");
let inheritedNameCalls = 0;
Object.defineProperty(Function.prototype, "name", { value: () => { inheritedNameCalls++; return "wrong"; }, configurable: true });
function shadowedName() {}
function ignoredArgument() { return 1; }
function callOwnName(f: any) {
  try { f.name(ignoredArgument()); }
  catch (e: any) {}
}
callOwnName(shadowedName);
callOwnName(shadowedName);
console.log("implicit own name", inheritedNameCalls, shadowedName.name);
Object.defineProperty(Function.prototype, "name", oldProtoName!);
