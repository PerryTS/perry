import http from "node:http";
import { EventEmitter } from "node:events";
function Response(this: any) {
  http.ServerResponse.call(this, { method: "GET" });
  this.label = 3;
}
const middle: any = Object.create(http.ServerResponse.prototype);
Object.setPrototypeOf(Response.prototype, middle);
function read(o: any) { return o.label; }
function headers(o: any) { return o.getHeader("x-test"); }
function emit(o: any) { return o.emit("sample", 7); }
const a: any = new (Response as any)();
const b: any = new (Response as any)();
b.label = 5;
let total = 0, delivered = 0;
// Use the ordinary emitter registry: main forwards Response.on to a separate
// native registry, while its inherited stream emit reads ordinary _events.
EventEmitter.prototype.on.call(a, "sample", (v: number) => { delivered += v; });
a.setHeader("x-test", "ok");
for (let i = 0; i < 1000; i++) {
  b.scratch = { i }; // Keep allocation pressure while the holder sites are warm.
  total += read(a) + read(b); headers(a); emit(a);
}
console.log("warm", total, delivered, headers(a));
let multi = 0;
EventEmitter.prototype.on.call(a, "multi", (x: number, y: number) => { multi += x + y; });
const detached = a.emit;
detached.call(a, "multi", 3, 4);
detached.apply(a, ["multi", 5, 6]);
console.log("emit arguments", multi, a.emit.length);
a.label = undefined;
let undefinedReads = 0;
for (let i = 0; i < 100; i++) if (read(a) === undefined) undefinedReads++;
console.log("undefined", read(a), undefinedReads);
let alternatingUndefined = 0, alternatingSum = 0;
for (let i = 0; i < 100; i++) {
  if (read(a) === undefined) alternatingUndefined++;
  alternatingSum += read(b);
}
console.log("alternating undefined", alternatingUndefined, alternatingSum);
a.label = null;
console.log("null", read(a));
delete a.label;
middle.label = 11;
console.log("inherited", read(a));
middle.label = 13;
console.log("holder overwrite", read(a));
Object.defineProperty(middle, "label", { get() { return this === a ? 17 : 19; }, configurable: true });
console.log("getter", read(a), read(b));
let getterCalls = 0;
Object.defineProperty(middle, "label", { get() { getterCalls++; return undefined; }, configurable: true });
for (let i = 0; i < 100; i++) read(a);
console.log("undefined getter", read(a), getterCalls);
delete middle.label;
let absentReads = 0;
for (let i = 0; i < 100; i++) if (read(a) === undefined) absentReads++;
console.log("absent", read(a), absentReads);
middle.getHeader = function() { return "patched"; };
console.log("method shadow", headers(a));
delete middle.getHeader;
console.log("method unshadow", headers(a));
let traps = 0;
Object.setPrototypeOf(Response.prototype, new Proxy(middle, {
  get(t, k, r) { traps++; return Reflect.get(t, k, r); },
}));
console.log("proxy", headers(a), traps);
