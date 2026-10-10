const call = Function.prototype.call;
const apply = Function.prototype.apply;
const bind = Function.prototype.bind;
function sum(this: any, a: number, b: number, c: number): string {
  "use strict";
  return String(this?.tag) + ":" + [a, b, c].join(",");
}
const c = call.bind(sum);
const a = apply.bind(sum);
const r = { tag: "R" };
console.log(c(r, 1, 2, 3), a(r, [1, 2, 3]));
// Exercise a heap closure operand with an ordinary lexical capture.
function capturedAdapters(prefix: { value: string }): any[] {
  function captured(this: any, x: number, y: number): string {
    "use strict";
    return this.tag + ":" + prefix.value + ":" + x + "," + y;
  }
  return [call.bind(captured), apply.bind(captured)];
}
const captured = capturedAdapters({ value: "C" });
// Reuse the traced operand across explicit moving collections when available.
let repeated = "";
let capturedResult = "";
for (let i = 0; i < 8; i++) {
  if (typeof (globalThis as any).gc === "function") (globalThis as any).gc();
  repeated = c({ tag: "R" }, i, i + 1, i + 2);
  capturedResult = captured[0]({ tag: "R" }, i, i + 1)
    + "/" + captured[1]({ tag: "R" }, [i, i + 1]);
}
console.log("repeat", repeated);
console.log("captured", capturedResult);
console.log(Reflect.apply(c, { ignored: true }, [r, 4, 5, 6]));
console.log(Reflect.apply(a, undefined, [r, [4, 5, 6]]));
console.log(c(r), a(r, null), c(r, 1, 2, 3, 4, 5, 6, 7));
console.log(c.name, c.length, a.name, a.length, Object.getPrototypeOf(c) === Function.prototype);
// Own-property/descriptor/prototype transitions retain the internal binding.
(c as any).extra = 9;
console.log(c(r, 7, 8, 9), (c as any).extra, c.name);
Object.defineProperty(a, "extra", { get() { return 8; } });
console.log(a(r, [7, 8, 9]), (a as any).extra);
Object.setPrototypeOf(c, { marker: 42 });
console.log(c(r, 10, 11, 12), (c as any).marker);
const viaReflect = Reflect.apply(bind, call, [sum]);
console.log(viaReflect(r, 1, 2, 3));
console.log(call.bind(sum, r, 20)(21, 22));
console.log(apply.bind(sum, r)([20, 21, 22]));
console.log(call.bind(sum).bind({ ignored: true }, r, 30)(31, 32));
// CreateListFromArrayLike must snapshot getters and honor holes/wide lists.
let reads = "";
const like = { get length() { reads += "L"; return 3; },
  get 0() { reads += "0"; return 40; }, get 1() { reads += "1"; return 41; },
  get 2() { reads += "2"; return 42; } };
console.log(a(r, like), reads);
console.log(a(r, [50, , 52]), a(r, [60, 61, 62, 63, 64, 65]));
const packed = [70, 71, 72];
Object.defineProperty(packed, Symbol.iterator, { get() { throw new Error("apply must not get an iterator"); } });
console.log(a(r, packed), a(r, Object.freeze([73, 74, 75])));
const indexed = [0, 81, 82];
Object.defineProperty(indexed, "0", { get() { reads += "I"; return 80; } });
console.log(a(r, indexed), reads);
function strictReceiver(this: unknown): string { "use strict"; return typeof this; }
const strictCall = call.bind(strictReceiver);
console.log(strictCall(7), strictCall("s"), strictCall(null), strictCall(undefined));
const sloppy = new Function("return typeof this + ':' + (this instanceof Number)");
console.log(call.bind(sloppy)(7), apply.bind(sloppy)(7, []));
const methodOwner = { tag: "owner", method(a: number) { return this.tag + ":" + a; } };
console.log(call.bind(methodOwner.method)({ tag: "other" }, 77));
const arrow = (() => { const lexical = { tag: "lexical", make() { return (a: number) => this.tag + a; } }; return lexical.make(); })();
console.log(call.bind(arrow)({ tag: "ignored" }, 78));
const wm = new WeakMap<object, number>();
const k = {};
const set = call.bind(WeakMap.prototype.set);
const get = call.bind(WeakMap.prototype.get);
set(wm, k, 99); console.log(get(wm, k), get(wm, {}));
const prox = new Proxy(sum, { apply(t, receiver, args) { return "P:" + Reflect.apply(t, receiver, args); } });
console.log(call.bind(prox)(r, 1, 2, 3));
const saved = Function.prototype.call;
Function.prototype.call = function () { return "patched"; } as any;
console.log(viaReflect(r, 1, 2, 3));
Function.prototype.call = saved;
