// #11896: built-ins used as VALUES behave like their direct forms.
import { Buffer } from "node:buffer";
import * as ns from "node:buffer";
import bufdef from "node:buffer";

function attempt(f: () => unknown): string {
  try {
    return String(f());
  } catch (e: any) {
    return "threw " + e.constructor.name;
  }
}

// ---- BigInt / Symbol called as plain values --------------------------------
const B = globalThis["BigInt"];
console.log(B(3), typeof B(3), B("12345678901234567890"), B(true), B(10n), B(-0));
const S = Symbol;
const s1 = S("x");
const s2 = S("x");
console.log(typeof s1, s1.toString(), s1.description, s1 === s2, typeof S(), S().description, S(undefined).description);
console.log([1, 2, 3].map(BigInt));
console.log(["a", "b"].map(Symbol).map(String));
console.log(Reflect.apply(BigInt, undefined, [7]), Reflect.apply(Symbol, undefined, ["r"]).toString());
console.log(BigInt.call(null, 5), Symbol.call(null, "c").toString(), BigInt.apply(null, [6]));
const bound = BigInt.bind(null, 9);
console.log(bound());
console.log(attempt(() => B()), attempt(() => B(1.5)), attempt(() => B("zz")), attempt(() => B(Symbol.iterator)));
console.log(attempt(() => { const SS: any = S; return new SS(); }));
console.log(attempt(() => { const BB: any = B; return new BB(1); }));
console.log(B.name, B.length, S.name, S.length, B === BigInt, S === Symbol);
console.log(typeof B.asIntN, B.asUintN(8, 257n), typeof S.iterator, S.for("q") === Symbol.for("q"));
console.log(typeof B(1) === "bigint", B(2) + B(3), S("k") !== S("k"));

// ---- Map.groupBy / RegExp.escape read as values ----------------------------
console.log(typeof Map.groupBy, typeof RegExp.escape, Map.groupBy.length, RegExp.escape.length, Map.groupBy.name, RegExp.escape.name);
const mg = Map.groupBy;
const g = mg([1, 2, 3, 4, 5], (x: number) => (x % 2 ? "odd" : "even"));
console.log(g instanceof Map, JSON.stringify([...g.entries()]));
const mc: any = Map;
console.log(JSON.stringify([...mc.groupBy("aab", (c: string) => c)]));
console.log(JSON.stringify([...Map.groupBy([1, 2, 3], (x: number) => x > 1).keys()]));
const objKey = {};
console.log(Map.groupBy([1, 2], () => objKey).get(objKey));
console.log(JSON.stringify([...Map.groupBy([1, 2, 3, 4], (x: number, i: number) => i % 2)]));
console.log(attempt(() => mg(null as any, (x: any) => x)), attempt(() => mg([1], 5 as any)));
const re = RegExp.escape;
console.log(JSON.stringify([re("a.b*c"), re("1abc"), re(" -"), re(""), re("^$.*+?()[]{}|\\")]));
console.log(JSON.stringify(["x+y", "(z)"].map(RegExp.escape)));
console.log(attempt(() => re(5 as any)), JSON.stringify(Reflect.apply(RegExp.escape, undefined, ["[a]"])));
const rc: any = RegExp;
console.log(JSON.stringify(rc.escape("^$")), new RegExp(RegExp.escape("a.b")).test("a.b"), new RegExp(RegExp.escape("a.b")).test("axb"));

// ---- instanceof WeakRef / FinalizationRegistry ------------------------------
const w = new WeakRef({});
const f = new FinalizationRegistry(() => {});
console.log(w instanceof WeakRef, f instanceof FinalizationRegistry, w instanceof FinalizationRegistry, f instanceof WeakRef);
console.log({} instanceof WeakRef, new WeakMap() instanceof WeakRef, new Map() instanceof FinalizationRegistry, null instanceof WeakRef, 1 instanceof WeakRef);
const WR: any = WeakRef;
const FR: any = FinalizationRegistry;
console.log(w instanceof WR, f instanceof FR, ({}) instanceof WR, w instanceof Object, f instanceof Object);
console.log(w instanceof Request, f instanceof Headers, new Request("http://x") instanceof WeakRef, new Headers() instanceof FinalizationRegistry);
console.log(Object.prototype.toString.call(w), Object.prototype.toString.call(f));

// ---- Buffer.compare ---------------------------------------------------------
const a = Buffer.from("a");
const b = Buffer.from("b");
console.log(Buffer.compare(a, b), Buffer.compare(b, a), Buffer.compare(a, a));
console.log([b, a, Buffer.from("ab"), Buffer.alloc(0)].sort(Buffer.compare).map((x) => x.toString()));
const cmp = Buffer.compare;
console.log(typeof cmp, cmp(a, b));
console.log(Reflect.apply(Buffer.compare, Buffer, [b, a]));
console.log(Buffer.compare(new Uint8Array([1, 2]), new Uint8Array([1, 3])), Buffer.compare(Buffer.from([1, 2, 3]), Buffer.from([1, 2])));
console.log(ns.Buffer.compare(a, b), bufdef.Buffer.compare(b, a), ns.Buffer.from("zz").toString(), ns.Buffer.concat([a, b]).toString());
console.log(a.compare(b), b.compare(a));
