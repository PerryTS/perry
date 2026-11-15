// ArraySpeciesCreate for every receiver and every method that has it. An
// Array-subclass instance is an Array: `map`, `filter`, `flat` and `flatMap`
// (like `slice`, `splice` and `concat`) construct their result through the
// receiver's species, so it is an instance of the subclass (its constructor
// runs with the length), unless the subclass names another species
// (`Array`, `null`, `undefined`, another class). `flat` and `flatMap` do the
// same for a plain Array whose `constructor` names a species: an `Object`
// species bears a Number object, a typed array or a frozen Array rejects the
// first definition, a non-constructor throws. The copies that never consult
// a species (`toSorted`, `toReversed`, `with`, `Array.from`, spread) bear a
// plain Array. The result is filled by CreateDataPropertyOrThrow: a setter
// or getter of a subclass prototype at the index is not called.
// Printed: the error class, not its message (the messages differ from node).
function show(label: string, f: () => any): void {
  let r: string;
  try { r = String(f()); } catch (e: any) { r = "throw " + (e && e.constructor && e.constructor.name); }
  console.log(label + " -> " + r);
}
function d(r: any): string {
  let vals = ""; const n = Math.min(Number(r.length) || 0, 12);
  for (let i = 0; i < n; i++) vals += (i > 0 ? "," : "") + String(r[i]);
  return Object.prototype.toString.call(r) + " ctor=" + (r.constructor && r.constructor.name) + " isArray=" + Array.isArray(r) + " len=" + String(r.length) + " [" + vals + "] keys=" + Object.keys(r).join("|");
}
class Stack extends Array<number> { top(): number { return this[this.length - 1]; } }
const log: string[] = [];
class Logged extends Array<number> { constructor(...args: any[]) { super(...(args as [])); log.push("ctor(" + args.join(",") + ")"); } tag(): string { return "L" + this.length; } }
class ToArray extends Array<number> { static get [Symbol.species]() { return Array; } }
class ToNull extends Array<number> { static get [Symbol.species]() { return null as any; } }
class ToUndef extends Array<number> { static get [Symbol.species]() { return undefined as any; } }
class ToStack extends Array<number> { static get [Symbol.species]() { return Stack; } }
class Setter extends Array<number> { set 0(v: number) { log.push("set0(" + v + ")"); } }
class Sub2 extends Stack { extra(): string { return "x" + this.length; } }
function fill<T extends number[]>(a: T): T { a.push(1, 2, 3, 4); return a; }
const CLASSES: [string, () => any][] = [
  ["Stack", () => fill(new Stack())], ["Logged", () => fill(new Logged())], ["ToArray", () => fill(new ToArray())], ["ToNull", () => fill(new ToNull())],
  ["ToUndef", () => fill(new ToUndef())], ["ToStack", () => fill(new ToStack())], ["Setter", () => fill(new Setter() as any)], ["Sub2", () => fill(new Sub2())],
  ["plain", () => [1, 2, 3, 4]],
];
const M: [string, (a: any) => any][] = [
  ["map", (a) => a.map((x: number) => x * 2)], ["filter", (a) => a.filter((x: number) => x > 1)], ["slice", (a) => a.slice(1, 3)], ["splice", (a) => a.splice(1, 2)],
  ["concat", (a) => a.concat([9])], ["flat", (a) => a.flat()], ["flatMap", (a) => a.flatMap((x: number) => [x, x])],
  ["toSorted", (a) => a.toSorted()], ["toReversed", (a) => a.toReversed()], ["with", (a) => a.with(0, 7)],
  ["Array.from", (a) => Array.from(a)], ["spread", (a) => [...a]],
];
for (const [cn, mk] of CLASSES) {
  for (const [mn, m] of M) {
    show(cn + "." + mn, () => { log.length = 0; const a = mk(); log.length = 0; const r = m(a); return d(r) + " instStack=" + (r instanceof Stack) + " inst=" + (r instanceof a.constructor) + " top=" + (typeof r.top === "function" ? r.top() : "-") + " tag=" + (typeof r.tag === "function" ? r.tag() : "-") + " log=" + log.join(";"); });
  }
  show(cn + " typed map", () => { const a = mk(); const f = (s: Stack) => s.map((x) => x + 1); const r: any = f(a); return d(r) + " " + (r instanceof Stack); });
}
// typed receivers
function tmap(s: Stack): any { return s.map((x) => x * 3); }
function tfilter(s: Stack): any { return s.filter((x) => x > 2); }
function tflat(s: Stack): any { return (s as any).flat(); }
const st = fill(new Stack());
show("typed Stack map", () => { const r = tmap(st); return d(r) + " " + (r instanceof Stack) + " " + r.top(); });
show("typed Stack filter", () => { const r = tfilter(st); return d(r) + " " + (r instanceof Stack) + " " + r.top(); });
show("typed Stack flat", () => { const r = tflat(st); return d(r) + " " + (r instanceof Stack); });
// species on a plain array's constructor: flat/flatMap
function withSpecies(species: any): any[] { const a: any[] = [1, [2, 3], 4]; (a as any).constructor = { [Symbol.species]: species }; return a; }
const SP: [string, any][] = [["Object", Object], ["Stack", Stack], ["Float64Array", Float64Array], ["frozen", function () { return Object.freeze([]); }], ["undefined", undefined], ["null", null], ["Array", Array],
  ["number", 5], ["arrow", () => []], ["obj setter", function () { const o: any = {}; Object.defineProperty(o, "0", { set(v) { log.push("set0"); }, configurable: true }); return o; }], ["logs", function (this: any, n: any) { log.push("species(" + n + ")"); return []; }]];
for (const [sn, sp] of SP) {
  show("species " + sn + " flat", () => { log.length = 0; const r = (withSpecies(sp) as any).flat(); return d(r) + " log=" + log.join(";"); });
  show("species " + sn + " flat(2)", () => { log.length = 0; const r = (withSpecies(sp) as any).flat(2); return d(r) + " log=" + log.join(";"); });
  show("species " + sn + " flatMap", () => { log.length = 0; const r = (withSpecies(sp) as any).flatMap((x: any) => [x, 0]); return d(r) + " log=" + log.join(";"); });
}
show("flat holes", () => d([1, , [2, , 3], , [[4]]].flat()));
show("flat depth", () => d([1, [2, [3, [4, [5]]]]].flat(Infinity)) + " " + d([1, [2, [3]]].flat(0)) + " " + d(([1, [2]] as any).flat(-1)) + " " + d(([1, [2, [3]]] as any).flat("1")));
show("flatMap this/args", () => d([1, 2].flatMap(function (this: any, x: number, i: number, arr: number[]) { return [x, i, arr.length, this.k]; }, { k: 7 })));
show("flatMap non-callable", () => ([1] as any).flatMap(5));
// (g): frozen subclass result / index setter on a subclass used as species result
show("frozen species subclass", () => { class F extends Array<number> { constructor(n?: any) { super(); Object.freeze(this); } } const a: any = [1, 2, 3]; a.constructor = { [Symbol.species]: F }; return d(a.map((x: number) => x)); });
show("sealed species subclass", () => { class F extends Array<number> { constructor(n?: any) { super(); Object.seal(this); } } const a: any = [1, 2, 3]; a.constructor = { [Symbol.species]: F }; return d(a.slice()); });
show("setter species subclass", () => { log.length = 0; const a: any = [1, 2, 3]; a.constructor = { [Symbol.species]: Setter }; const r = a.map((x: number) => x * 5); return d(r) + " log=" + log.join(";") + " own0=" + JSON.stringify(Object.getOwnPropertyDescriptor(r, 0)); });
show("getter species subclass", () => { class G extends Array<number> { get 1() { return "g"; } } const a: any = [1, 2, 3]; a.constructor = { [Symbol.species]: G }; const r = a.filter(() => true); return d(r) + " own1=" + JSON.stringify(Object.getOwnPropertyDescriptor(r, 1)); });
show("nonext species subclass", () => { class F extends Array<number> { constructor(n?: any) { super(); Object.preventExtensions(this); } } const a: any = [1, 2, 3]; a.constructor = { [Symbol.species]: F }; return d(a.concat([4])); });
