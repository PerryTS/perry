// #11932: `fn.call` / `fn.apply` sites that run hot BEFORE the program ever
// touches %Function.prototype% (so the runtime has not built it), and then
// keep running after the program builds and patches it. Nothing in phase 1
// may name `Function`, `Object` or `Reflect`, or create a bound function
// (a bound function's [[Prototype]] is %Function.prototype%, so `bind`
// builds it).

const out: string[] = [];
function add(this: any, x: number, y: number) { return (this && this.base ? this.base : 0) + x + (y === undefined ? 0 : y); }
const holder: any = { base: 100 };
function run(f: any, t: any, i: number): any {
  return f.call(t, i, 1) + f.apply(t, [i, 2]);
}

// Phase 1: hot, realm not built.
let s = 0;
for (let i = 0; i < 1000; i++) s += run(add, holder, i);
out.push("phase1=" + s);
let u = 0;
for (let i = 0; i < 1000; i++) u += run(add, undefined, i);
out.push("phase1-undef=" + u);
const arrow = (x: number) => x * 3;
let a = 0;
for (let i = 0; i < 500; i++) a += arrow.call(null, i);
out.push("phase1-arrow=" + a);

// Phase 2: the program builds %Function.prototype% and patches `call` and
// `apply` through an alias with computed keys.
const FP: any = Object.getPrototypeOf(add);
const kCall = "ca" + "ll";
const kApply = "app" + "ly";
const orig = { call: FP[kCall], apply: FP[kApply] };
let calls = 0;
FP[kCall] = function (this: any, t: any, ...rest: any[]) { calls++; return Reflect.apply(this, t, rest) + 0.5; };
FP[kApply] = function (this: any, t: any, list: any) { calls++; return Reflect.apply(this, t, list) + 0.25; };
let p = 0;
for (let i = 0; i < 1000; i++) p += run(add, holder, i);
let pa = 0;
for (let i = 0; i < 500; i++) pa += arrow.call(null, i);
out.push("phase2-patched=" + p + " arrow=" + pa + " calls=" + calls);

// Phase 3: restored, hot again, and `bind` now too.
FP[kCall] = orig.call;
FP[kApply] = orig.apply;
calls = 0;
let r = 0;
for (let i = 0; i < 1000; i++) r += run(add, holder, i) + add.bind(holder, i)(3);
out.push("phase3-restored=" + r + " calls=" + calls);
console.log(out.join("\n"));
