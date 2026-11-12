// ArraySpeciesCreate with a species that is not `%Array%`: `slice`, `splice`,
// `concat`, `map` and `filter` fill whatever the species constructor returns
// with CreateDataPropertyOrThrow (the object's own [[DefineOwnProperty]]: no
// setter runs, a frozen or non-extensible result throws), and `slice`,
// `splice` and `concat` end with Set(A, "length", n, true). Only the default
// species (`%Array%`, undefined, null) bears a plain Array.
const log: string[] = [];

function describe(r: any): string {
  const keys = Object.getOwnPropertyNames(r).join(",");
  let vals = "";
  const n = Math.min(Number(r.length) || 0, 14);
  for (let i = 0; i < n; i++) vals += (i > 0 ? "," : "") + String(r[i]);
  return (
    Object.prototype.toString.call(r) +
    " isArray=" + Array.isArray(r) +
    " length=" + String(r.length) +
    " keys=" + keys +
    " [" + vals + "]"
  );
}

function withSpecies(species: any): any[] {
  const a: any[] = [1, 2, 3, 4, 5];
  (a as any).constructor = { [Symbol.species]: species };
  return a;
}

const methods: [string, (a: any[]) => any][] = [
  ["slice", (a) => a.slice(1, 4)],
  ["splice", (a) => a.splice(1, 3)],
  ["concat", (a) => a.concat([9, 8], 7)],
  ["map", (a) => a.map((x: number) => x * 2)],
  ["filter", (a) => a.filter((x: number) => x % 2 === 1)],
];

function run(name: string, species: any): void {
  for (const [mn, m] of methods) {
    log.length = 0;
    const a = withSpecies(species);
    try {
      const r = m(a);
      console.log(name, mn, "->", describe(r), "log=" + log.join("|"), "src=" + a.join(","));
    } catch (e: any) {
      console.log(name, mn, "threw", e instanceof TypeError ? "TypeError" : String(e), "log=" + log.join("|"), "srclen=" + a.length);
    }
  }
}

// (a) Realm constructors that are not `%Array%`. `new Object(count)` is a
// Number object; a typed array has no settable `length`, and one of length 0
// has no index to define.
run("Object", Object);
run("Float64Array", Float64Array);
run("Uint8Array", Uint8Array);

// (b) A species constructor returning an Array that is frozen, sealed, not
// extensible, shorter or longer than the count.
run("frozenEmpty", function (_n: number) { return Object.freeze([]); });
run("frozenFull", function (_n: number) { return Object.freeze([0, 0, 0, 0, 0, 0, 0, 0, 0, 0]); });
run("sealedFull", function (_n: number) { return Object.seal([0, 0, 0, 0, 0, 0, 0, 0, 0, 0]); });
run("noExtendEmpty", function (_n: number) { return Object.preventExtensions([]); });
run("short", function (_n: number) { return []; });
run("long", function (_n: number) { return [7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7]; });
run("sized", function (n: number) { return new Array(n); });

// (c) A species constructor returning an ordinary object: a setter on an
// index is replaced, never called; a `length` setter is called; a read-only
// or getter-only `length` and a non-configurable index throw.
run("setter", function (_n: number) {
  const o: any = {};
  Object.defineProperty(o, "1", { set(v) { log.push("idx-set " + v); }, get() { return "g"; }, configurable: true, enumerable: true });
  Object.defineProperty(o, "length", { set(v) { log.push("len-set " + v); }, get() { return 99; }, configurable: true });
  return o;
});
run("fixedIndex", function (_n: number) {
  const o: any = {};
  Object.defineProperty(o, "0", { value: "fixed", writable: true, enumerable: true, configurable: false });
  return o;
});
run("readOnlyLength", function (_n: number) {
  const o: any = {};
  Object.defineProperty(o, "length", { value: 77, writable: false, configurable: true });
  return o;
});
run("getterOnlyLength", function (_n: number) {
  const o: any = {};
  Object.defineProperty(o, "length", { get() { return 5; }, configurable: true });
  return o;
});
run("frozenObject", function (_n: number) { return Object.freeze({}); });
run("plainCtor", function (this: any, n: number) { this.n = n; });

// (d) Negative controls: the default species, an Array subclass, and species
// values that are no constructor.
class Stack extends Array<number> {
  top(): number {
    return this[this.length - 1] ?? -1;
  }
}
run("undefined", undefined);
run("null", null);
run("Array", Array);
run("Stack", Stack);
run("number", 5);
run("string", "x");
run("arrow", () => []);
run("Math.max", Math.max);

const st = new Stack();
st.push(4, 8, 15, 16, 23, 42);
const cut = st.slice(1, 4);
const dbl = st.map((x) => x * 2);
const big = st.filter((x) => x > 10);
const cat = st.concat([1, 2]);
const gone = st.splice(0, 2);
console.log("stack", cut instanceof Stack, cut.length, cut.top(), dbl instanceof Stack, dbl.length, dbl.top(), big instanceof Stack, big.length, big.top());
console.log("stack2", cat instanceof Stack, cat.length, cat.top(), gone instanceof Stack, gone.length, gone.top(), st.length, st.top());

// (e) Holes stay absent on a species result; `length` still counts them.
const holey: any[] = [1, , 3, , 5];
(holey as any).constructor = { [Symbol.species]: function (this: any, _n: number) { this.tag = "t"; } };
const hs: any = holey.slice(0, 4);
const hc: any = holey.concat([, 6]);
const hm: any = holey.map((x: number) => x + 1);
console.log("holes", describe(hs), "|", describe(hc), "|", describe(hm), 1 in hs, 3 in hc, 5 in hc, 1 in hm);
const hp: any = holey.splice(0, 3);
console.log("holes splice", describe(hp), 1 in hp, holey.length);

// (f) The species is resolved before any element is read or the callback
// runs, and a throwing constructor leaves the receiver untouched.
const order: string[] = [];
const watched: any[] = [1, 2, 3];
Object.defineProperty(watched, "constructor", {
  get() {
    order.push("constructor");
    return { get [Symbol.species]() { order.push("species"); return function (n: number) { order.push("construct " + n); return {}; }; } };
  },
});
const wm: any = watched.map((x: number) => { order.push("cb " + x); return x; });
console.log("order", order.join(" > "), describe(wm));
const boom: any[] = [1, 2, 3, 4];
(boom as any).constructor = { [Symbol.species]: function (_n: number) { throw new RangeError("no"); } };
for (const [mn, m] of methods) {
  try {
    m(boom);
    console.log("boom", mn, "no throw");
  } catch (e: any) {
    console.log("boom", mn, e instanceof RangeError ? "RangeError" : String(e), boom.join(","));
  }
}

// (g) The generic form on an array-like never consults a species; on a real
// Array with a species it does.
const like = { length: 3, 0: "a", 1: "b", 2: "c", constructor: { [Symbol.species]: Object } };
const fromLike: any = Array.prototype.slice.call(like, 1);
console.log("arraylike", describe(fromLike));
const viaCall: any = Array.prototype.slice.call(withSpecies(Object), 2);
console.log("call", describe(viaCall));

// (h) Many copies through one site: the default species next to an own
// `constructor` that still resolves to `%Array%`.
let sum = 0;
for (let r = 0; r < 200; r++) {
  const plain: number[] = [r, r + 1, r + 2, r + 3];
  const own: number[] = [r, r + 1, r + 2, r + 3];
  if (r % 2 === 0) (own as any).constructor = Array;
  const p = plain.slice(1);
  const o = own.slice(1, 3);
  p.push(1);
  o[0] = 2;
  sum += p.length * 3 + (p[0] ?? 0) + o.length + (o[0] ?? 0) + (o[1] ?? 0) + (Array.isArray(o) ? 1 : 0);
}
console.log("sum", sum);
