// Function.prototype.call and .bind forward their own argument list to the
// target in place. Pin every shape of that forwarding against Node: the
// uncurried `call.bind(method)` idiom (call-bound / side-channel / qs), zero
// and many arguments, primitive receivers, object-literal methods that keep
// `this` in a capture, proxies, and call/bind reached as values.

const $call = Function.prototype.call;
const $bind = Function.prototype.bind;
const $apply = Function.prototype.apply;

console.log($call.length, $bind.length, $call.name, $bind.name);

// call-bind-apply-helpers: Reflect.apply(bind, call, [fn]) uncurries `fn`.
const uncurry = (fn: Function): any => Reflect.apply($bind, $call, [fn]);
const wmGet = uncurry(WeakMap.prototype.get);
const wmSet = uncurry(WeakMap.prototype.set);
const wmHas = uncurry(WeakMap.prototype.has);
const indexOf = uncurry(String.prototype.indexOf);
const wm = new WeakMap<object, unknown>();
const keys: object[] = [];
for (let i = 0; i < 1000; i++) {
  const k = { i };
  keys.push(k);
  wmSet(wm, k, "v" + i);
}
let hits = 0;
for (let i = 0; i < 1000; i++) {
  if (wmHas(wm, keys[i]) && wmGet(wm, keys[i]) === "v" + i) hits++;
}
console.log(hits, wmGet(wm, {}), indexOf("a.prototype.b", ".prototype."), indexOf("abc", "c", 1));

// call with no arguments at all, and with only a receiver.
function who(this: any, ...rest: unknown[]): string {
  return (this === undefined ? "undef" : typeof this) + "/" + rest.length;
}
console.log($call.call(who), $call.call(who, { x: 1 }), who.call(undefined, 1, 2, 3));

// Wide argument lists through call and through bound call.
function sum(this: any, ...xs: number[]): number {
  let s = this && typeof this.base === "number" ? this.base : 0;
  for (const x of xs) s += x;
  return s;
}
const wide: number[] = [];
for (let i = 1; i <= 40; i++) wide.push(i);
console.log(sum.call({ base: 1000 }, ...wide), uncurry(sum)({ base: 1 }, ...wide));

// Fixed-arity callee under- and over-applied through call.
function three(this: any, a: unknown, b: unknown, c: unknown): string {
  return [this?.tag, a, b, c].map(String).join(",");
}
console.log(three.call({ tag: "t" }), three.call({ tag: "t" }, 1), three.call({ tag: "t" }, 1, 2, 3, 4, 5));

// Sloppy-mode primitive receivers are boxed; strict ones are not.
const sloppyThis = new Function("return typeof this + ':' + (this instanceof Number);");
function strictThis(this: unknown): string {
  "use strict";
  return typeof this;
}
console.log(sloppyThis.call(5), strictThis.call(5), strictThis.call("s"), $call.call(strictThis, true));
const boxed = new Function("this.touched = true; return this;").call(7);
console.log(typeof boxed, boxed.touched, boxed + 1);

// Object-literal methods keep `this` in a capture: call must still rebind.
const objA = {
  name: "A",
  hello(greeting: string, punct: string) {
    return greeting + " " + this.name + punct;
  },
};
const objB = { name: "B" };
console.log(objA.hello.call(objB, "hi", "!"), uncurry(objA.hello)(objB, "yo", "?"));
const helloB = objA.hello.bind(objB, "bound");
console.log(helloB("."), helloB.length, helloB.name);

// Proxies as call targets keep the explicit receiver.
const target = function (this: any, x: number, y: number) {
  return (this && this.k) + ":" + (x + y);
};
const prox = new Proxy(target, {
  apply(t, thisArg, args) {
    return "proxied(" + Reflect.apply(t, thisArg, args) + ")";
  },
});
console.log(prox.call({ k: "P" }, 2, 3), uncurry(prox)({ k: "Q" }, 4, 5));

// bind reached through call/apply/Reflect.apply, with and without partials.
function tagged(this: any, a: unknown, b: unknown): string {
  return this.t + "|" + a + "|" + b;
}
const b1 = $bind.call(tagged, { t: "b1" }, "x");
const b2 = $bind.apply(tagged, [{ t: "b2" }, "y", "z"]);
const b3 = Reflect.apply($bind, tagged, [{ t: "b3" }]);
const b4 = ($bind as any).call(tagged);
console.log(b1("q"), b2(), b3(1, 2), b1.length, b2.length, b3.length, b1.name);
console.log(typeof b4, b4.length, b4.name);

// call on a non-callable throws a TypeError.
try {
  ($call as any).call({}, 1);
} catch (err) {
  console.log("noncallable", (err as Error).constructor.name);
}

// apply forwarding the same way.
console.log($apply.call(sum, { base: 10 }, [1, 2, 3]), Reflect.apply($call, sum, [{ base: 5 }, 1, 2]));

// Heap-allocated arguments survive allocation inside the forwarded call.
function build(this: any, ...parts: object[]): string {
  const junk: number[][] = [];
  for (let i = 0; i < 2000; i++) junk.push([i, i + 1]);
  return parts.map((p: any) => p.v).join("+") + "#" + junk.length + "#" + this.v;
}
const parts: object[] = [];
for (let i = 0; i < 12; i++) parts.push({ v: "p" + i });
let ok = 0;
for (let r = 0; r < 50; r++) {
  const out = uncurry(build)({ v: "self" + r }, ...parts);
  if (out === parts.map((p: any) => p.v).join("+") + "#2000#self" + r) ok++;
}
console.log("survived", ok);
