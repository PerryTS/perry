// #11301: builtin namespace members read as VALUES, not just called.
//
// cc (claude-code 2.1.112) died at startup because lodash's
// `nativeMax = Math.max` read `undefined` inside an esbuild `__esm` thunk.
// Direct calls (`Math.max(1, 2)`) lower to intrinsics and never touch the
// namespace object, so only the value forms below can see a namespace whose
// members were never installed. The cause was the no-auto HTTP rebuild
// (#11174): importing `http` rebuilt the stdlib archive with a bundled runtime
// that lacked perry-runtime's default features, and that copy's globalThis
// bootstrap ran with the per-namespace member tables (`global-math`,
// `global-json`, ...) compiled out. The parity runner pins this fixture to
// PERRY_NO_AUTO_OPTIMIZE=1 so it keeps exercising that exact link.
import * as http from "node:http";

console.log("http", typeof http.createServer);

// --- the cc shape: an esbuild-style lazy init assigns the value in a nested closure
const lazy = (fn: any, res?: any) => () => (fn && (res = fn((fn = 0))), res);
let nativeMax: any, nativeMin: any, overRest: any;
const initMath = lazy(() => {
  nativeMax = Math.max;
  nativeMin = Math.min;
  overRest = function (func: any, start?: number) {
    start = nativeMax(start === undefined ? func.length - 1 : start, 0);
    return function (this: any, ...args: any[]) {
      return func.apply(this, args.slice(0, nativeMin(args.length, 8)));
    };
  };
});
function outer() {
  return function middle() {
    return function inner() {
      initMath();
      return overRest((a: number, b: number) => a + b)(3, 4);
    };
  };
}
console.log("cc-shape", outer()()(), typeof nativeMax, typeof nativeMin);

// --- per namespace: direct call, then the same member read as a value and called
const m = Math.max;
console.log("Math", Math.max(1, 2), m(3, 9), typeof Math.abs, Math.PI.toFixed(3));
const parse = JSON.parse;
const stringify = JSON.stringify;
console.log("JSON", JSON.stringify({ a: 1 }), stringify(parse('{"b":2}')));
const ownKeys = Reflect.ownKeys;
const rget = Reflect.get;
console.log("Reflect", Reflect.ownKeys({ x: 1 }).length, ownKeys({ p: 1, q: 2 }).length, rget({ k: 7 }, "k"));
const ia = new Int32Array(new SharedArrayBuffer(8));
const aadd = Atomics.add;
const aload = Atomics.load;
Atomics.add(ia, 0, 2);
aadd(ia, 0, 3);
console.log("Atomics", aload(ia, 0));
const okeys = Object.keys;
const oassign = Object.assign;
console.log("Object", Object.keys({ a: 1 }).join(), okeys(oassign({}, { z: 1 })).join());
const isInt = Number.isInteger;
console.log("Number", Number.isInteger(2), isInt(2.5), Number.MAX_SAFE_INTEGER);
const fcc = String.fromCharCode;
console.log("String", String.fromCharCode(72), fcc(105));
const isArr = Array.isArray;
const afrom = Array.from;
console.log("Array", Array.isArray([]), isArr({}), afrom("ab").join("-"));
const pall = Promise.all;
console.log("Promise", typeof Promise.all, typeof pall, typeof Promise.resolve);
const sfor = Symbol.for;
console.log("Symbol", typeof Symbol.iterator, sfor("k") === Symbol.for("k"));
const asU = BigInt.asUintN;
console.log("BigInt", BigInt.asUintN(8, 257n), asU(8, 258n));
const dnow = Date.now;
console.log("Date", typeof Date.now(), typeof dnow());
const cst = Error.captureStackTrace;
console.log("Error", typeof Error.captureStackTrace, typeof cst);
const NF = Intl.NumberFormat;
console.log("Intl", typeof Intl.NumberFormat, new NF("en-US").format(1234));
const pInt = parseInt;
const enc = encodeURIComponent;
console.log("globals", pInt("42"), enc("a b"), typeof globalThis.parseFloat);

// --- escaped / dynamically keyed namespace reads
const keys = ["max", "min", "abs", "floor"];
const M: any = Math;
console.log("Math[k]", keys.map((k) => typeof (Math as any)[k]).join(), M[keys[0]](4, 8), M["abs"](-3));
const J: any = JSON;
console.log("JSON[k]", ["parse", "stringify"].map((k) => typeof J[k]).join());
const R: any = Reflect;
console.log("Reflect[k]", ["apply", "has", "construct"].map((k) => typeof R[k]).join());
console.log("own", Object.getOwnPropertyNames(Math).includes("max"), Object.prototype.hasOwnProperty.call(JSON, "parse"));
console.log("reflect-get", typeof Reflect.get(Math, "min"), typeof Reflect.get(Reflect, "ownKeys"));
const g: any = globalThis;
const names = ["Math", "JSON", "Reflect", "Atomics"];
console.log("globalThis[name]", names.map((n) => typeof g[n]).join(), g["Math"].max(1, 5), typeof g.JSON.stringify);
function takesNamespace(ns: any, member: string) {
  return typeof ns[member];
}
console.log("escaped", takesNamespace(Math, "round"), takesNamespace(JSON, "parse"), takesNamespace(Reflect, "ownKeys"));

// --- the process/console value surfaces (installed on demand since the size work)
const c: any = console;
c["log"]("console-value", typeof c.error);
const p: any = process;
console.log("process-value", typeof p.cwd, typeof p.stdout.write, typeof g.process.nextTick);
// Same campaign: process metadata behind its registry (5fde6f6e09) and
// localeCompare without the Intl namespace (#11114).
const metaKey = ["execArgv", "report"][0];
console.log("process-meta", Array.isArray(p.execArgv), Array.isArray(p[metaKey]), typeof process.report);
const Coll = Intl.Collator;
console.log("locale", "a".localeCompare("b"), new Coll("en").compare("b", "a"), typeof g.Intl.DateTimeFormat);
