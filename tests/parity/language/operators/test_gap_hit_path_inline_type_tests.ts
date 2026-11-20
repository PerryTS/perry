// Semantics pinned for the hit-path inline lowerings: typed/specialized entry
// guards (plain doubles first, int32 boxes second), guarded boolean
// truthiness, typeof literal comparisons, switch literal cases, two-argument
// Math.max/min, module-global ++/--, and medium string-literal equality.
// Every value family each inline arm decides by bits is exercised next to the
// ones that must still reach the runtime helper.

class Klass {
  x = 1;
}
function plainFn(): number {
  return 1;
}
const arrow = () => 2;
const sym = Symbol("s");
const big = 10n;
const date = new Date(0);
const f64 = new Float64Array(2);
const heapStr = ["hello", "-", "world"].join("");
const ssoStr = ["a", "b"].join("");
const valueOfObj = { valueOf: () => 41 };

const values: any[] = [
  undefined, null, true, false, 0, -0, 1.5, -2, NaN, Infinity, -Infinity,
  2 ** 31, 1048576, 1048577.5, 2097151, "", "ab", heapStr, ssoStr, sym, big,
  {}, [], plainFn, arrow, Klass, new Klass(), date, f64, valueOfObj,
];

// ---- typeof: strict, loose, negated, and non-local operands ----
const literals = ["undefined", "object", "boolean", "number", "string", "function", "symbol", "bigint"];
function typeofRow(v: any): string {
  let out = "";
  for (const lit of literals) {
    let hit = false;
    switch (lit) {
      case "undefined": hit = typeof v === "undefined"; break;
      case "object": hit = typeof v === "object"; break;
      case "boolean": hit = typeof v === "boolean"; break;
      case "number": hit = typeof v === "number"; break;
      case "string": hit = typeof v === "string"; break;
      case "function": hit = typeof v === "function"; break;
      case "symbol": hit = typeof v === "symbol"; break;
      case "bigint": hit = typeof v === "bigint"; break;
    }
    out += hit ? "1" : "0";
  }
  const holder = { a: v };
  out += "|" + (typeof holder.a == "string" ? "s" : "-") + (typeof holder.a != "object" ? "n" : "o");
  out += (typeof v !== "function" ? "F" : "f") + (typeof v != "undefined" ? "D" : "u");
  return out;
}
for (let i = 0; i < values.length; i++) console.log("typeof", i, typeofRow(values[i]));
const sparse: any[] = [1, , 3];
console.log("typeof hole", typeof sparse[1] === "undefined", typeof sparse[1]);

// ---- switch: short numeric, string literals, singletons, mixed, fall-through ----
function numSwitch(v: any): string {
  switch (v) {
    case 0: return "zero";
    case 1: return "one";
    case -3: return "minus-three";
    default: return "other";
  }
}
function strSwitch(v: any): string {
  switch (v) {
    case "ab": return "sso";
    case "hello-world": return "heap";
    case "": return "empty";
    case "sixteen-bytes-ok": return "sixteen";
    case "seventeen-bytes!!": return "seventeen";
    default: return "none";
  }
}
function mixedSwitch(v: any): string {
  let r = "";
  switch (v) {
    case null: r += "null";
    case undefined: r += "undef"; break;
    case true: r += "true"; break;
    case 7: r += "seven"; break;
    case "7": r += "str7"; break;
    case false: r += "false"; break;
    default: r += "dflt";
    case 8: r += "eight";
  }
  return r;
}
const dup = (v: number): string => {
  switch (v) {
    case 2: return "first";
    case 2: return "second";
    default: return "d";
  }
};
function toI(x: number): number {
  return x | 0;
}
const int32Box = toI(7.9);
const probes: any[] = [0, -0, 1, 1.0, -3, 7, "7", 8, NaN, int32Box, toI(-3.2), "ab", heapStr, ssoStr,
  "", "sixteen-bytes-ok", "seventeen-bytes!!", ["sixteen-bytes-o", "k"].join(""), null, undefined, true, false, {}];
for (let i = 0; i < probes.length; i++) {
  console.log("switch", i, numSwitch(probes[i]), strSwitch(probes[i]), mixedSwitch(probes[i]));
}
console.log("switch dup", dup(2), dup(3));

// ---- string literal equality, 4..=16 bytes and beyond ----
const eqInputs: any[] = ["dest", "destroy", "hello-world", "hello-worle", "jello-world", "sixteen-bytes-ok",
  "sixteen-bytes-oK", "seventeen-bytes!!", heapStr, "héllo-wörld", 12, null];
for (const s of eqInputs) {
  console.log("streq", s === "dest", s === "destroy", s === "hello-world", s !== "sixteen-bytes-ok",
    s === "seventeen-bytes!!", s === "héllo-wörld");
}

// ---- Math.max / Math.min with two arguments ----
const mm: any[] = [1, -1, 0, -0, NaN, Infinity, -Infinity, 1.5, "3", null, undefined, true, int32Box, valueOfObj];
for (const a of mm) {
  let row = "";
  for (const b of mm) {
    const hi = Math.max(a, b);
    const lo = Math.min(a, b);
    row += `${Object.is(hi, -0) ? "-0" : hi},${Object.is(lo, -0) ? "-0" : lo};`;
  }
  console.log("minmax", String(a), row);
}
console.log("floor/sqrt", Math.floor(int32Box), Math.sqrt(int32Box as any), Math.floor("2.5" as any), Math.sqrt(null as any));

// ---- module-global ++/-- across value families ----
let gNum: any = 1.5;
let gInt: any = toI(5.5);
let gBig: any = 41n;
let gStr: any = "9";
let gObj: any = valueOfObj;
let gUndef: any = undefined;
function bump(): void {
  gNum++; gInt++; gBig++; gStr++; gObj++; gUndef++;
  --gNum; --gInt;
}
bump();
console.log("globals", gNum, gInt, gBig, gStr, gObj, gUndef, typeof gBig, gNum++ + ++gNum);

// ---- typed and specialized entries: plain doubles, int32 boxes, lies ----
export function addNums(a: number, b: number): number {
  return a + b;
}
export function pickIf(v: any, flag: boolean): any {
  if (flag) {
    return v;
  }
  return "no";
}
export function notFlag(flag: boolean): boolean {
  return !flag;
}
export function strLenOf(s: string): number {
  return s.length;
}
export function labelOf(o: { label: number }): number {
  return o.label;
}
const addRef: any = addNums;
const pickRef: any = pickIf;
const notRef: any = notFlag;
const lenRef: any = strLenOf;
const labelRef: any = labelOf;
console.log("entry add", addRef(1.5, 2), addRef(int32Box, 0.5), addRef(int32Box, int32Box), addRef("a", 1), addRef(undefined, 1));
console.log("entry pick", pickRef(1, true), pickRef(1, false), pickRef(1, 0), pickRef(1, "x"), pickRef(1, undefined), pickRef(1, {}));
console.log("entry not", notRef(true), notRef(false), notRef(0), notRef(""), notRef("x"));
console.log("entry len", lenRef("abc"), lenRef(ssoStr), lenRef(heapStr), lenRef([1, 2]));
console.log("entry label", labelRef({ label: 3 }), labelRef({ label: "x" }), labelRef({}));
