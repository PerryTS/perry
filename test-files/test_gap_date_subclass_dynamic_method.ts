// A dynamic method call on an instance of `class D extends Date` resolves the
// method on the instance's own prototype chain: D.prototype first, then
// Date.prototype. (It used to read the realm's Date.prototype directly.)
class D extends Date {
  getTime(): any { return "D.getTime"; }
  label() { return "D.label " + this.getUTCDate(); }
}
class E extends D {
  toISOString(): any { return "E.toISOString"; }
}
const name = (...parts: string[]) => parts.join("");
function call(x: any, m: string, ...args: any[]) { return x[m](...args); }
function untyped(x: any) { return [x.getTime(), x.toISOString()]; }
const d = new D(86400000);
const e = new E(0);
console.log(call(d, name("get", "Time")), call(d, "getUTCDate"), call(d, "label"));
console.log(call(e, "getTime"), call(e, "toISOString"), call(e, "getUTCFullYear"));
console.log(untyped(d)[0], untyped(e)[1], untyped(new Date(0))[1]);
console.log(Reflect.apply((d as any)[name("get", "Time")], d, []), call(d, "setUTCHours", 3));
const own: any = new D(0);
own.getTime = () => "own";
console.log(call(own, "getTime"), untyped(own)[0]);
