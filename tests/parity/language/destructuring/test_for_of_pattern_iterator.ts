// A for-of / for-await head pattern binds its element like the same pattern
// in a declaration: an array pattern drives the element's own iterator
// (`Symbol.iterator`, `next`, IteratorClose when the pattern stops early), an
// object pattern takes numeric and string keys. Sources: an iterator object,
// a generator, a Set, a Map, a string, an arguments object, a typed array, a
// Map key iterator, an Array iterator, a custom iterable; holes, defaults,
// rest, nesting, assignment heads. Negative controls: plain Arrays and
// tuples, non-iterable elements (TypeError).
const log: string[] = [];
function mkIt(vals: any[]): any {
  let i = 0;
  return { [Symbol.iterator]() { log.push("iter"); return this; },
    next() { log.push("next"); return i < vals.length ? { done: false, value: vals[i++] } : { done: true, value: undefined }; },
    return() { log.push("return"); return {}; } };
}
function* gen() { try { yield 1; yield 2; yield 3; } finally { log.push("gen-finally"); } }
function args(..._a: any[]) { return arguments; }
function out(label: string, f: () => any) { log.length = 0; let r: any; try { r = f(); } catch (e: any) { r = "THROW " + e.constructor.name; } console.log(label, JSON.stringify(r), log.join("|")); }
const sources: [string, () => any][] = [
  ["iterobj", () => mkIt([7, 8, 9])], ["gen", () => gen()], ["set", () => new Set([4, 5, 6])],
  ["map", () => new Map([[1, "a"], [2, "b"]])], ["str", () => "xyz"], ["args", () => args(1, 2, 3)],
  ["u8", () => new Uint8Array([10, 11, 12])], ["arr", () => [1, 2, 3]], ["mapkeys", () => new Map([[1, 2]]).keys()],
  ["arrIter", () => [5, 6].values()], ["custom", () => ({ *[Symbol.iterator]() { yield "c1"; yield "c2"; } })],
];
for (const [n, mk] of sources) {
  out("forof1 " + n, () => { const r: any[] = []; for (const [p] of [mk()]) r.push(p); return r; });
  out("forof2 " + n, () => { const r: any[] = []; for (const [p, , q] of [mk()]) r.push(p, q); return r; });
  out("forofrest " + n, () => { const r: any[] = []; for (const [p, ...rest] of [mk()]) r.push(p, rest); return r; });
  out("forofdef " + n, () => { const r: any[] = []; for (const [a, b, c, d = "D"] of [mk()]) r.push(a, b, c, d); return r; });
  out("forofnest " + n, () => { const r: any[] = []; for (const [[p]] of [[mk()]]) r.push(p); return r; });
  out("forofassign " + n, () => { let p: any, q: any; const r: any[] = []; for ([p, q] of [mk()]) r.push(p, q); return r; });
  out("forofletobj " + n, () => { const r: any[] = []; for (let { 0: z } of [mk()] as any[]) r.push(z); return r; });
  out("decl " + n, () => { const [a, b] = mk(); return [a, b]; });
  out("declhole " + n, () => { const [, b] = mk(); return [b]; });
  out("declrest " + n, () => { const [a, ...r] = mk(); return [a, r]; });
  out("assign " + n, () => { let a: any, b: any; [a, b] = mk(); return [a, b]; });
  out("param " + n, () => { const f = ([a, b]: any) => [a, b]; return f(mk()); });
  out("paramdef " + n, () => { function f([a, b] : any = mk()) { return [a, b]; } return f(); });
  out("nestobj " + n, () => { const { k: [a] } = { k: mk() }; return [a]; });
  out("forofmap " + n, () => { const r: any[] = []; for (const [a, b] of new Map([[mk(), 1]]).keys() as any) r.push(a, b); return r; });
  out("forofarrvar " + n, () => { const r: any[] = []; const arr = [mk(), mk()]; for (const [p] of arr) r.push(p); return r; });
}
out("forof elems mixed", () => { const r: any[] = []; for (const [a, b] of [[1, 2], "ab", new Set([3, 4]), mkIt([5, 6])] as any[]) r.push(a, b); return r; });
out("forof nonIterable", () => { const r: any[] = []; for (const [a] of [{}] as any[]) r.push(a); return r; });
out("forof null", () => { const r: any[] = []; for (const [a] of [null] as any[]) r.push(a); return r; });
out("forof number", () => { const r: any[] = []; for (const [a] of [5] as any[]) r.push(a); return r; });
out("forof arraylike", () => { const r: any[] = []; for (const [a] of [{ 0: "x", length: 1 }] as any[]) r.push(a); return r; });
out("forof arr with own iter", () => { const x: any = [1, 2]; x[Symbol.iterator] = function* () { yield "own"; }; const r: any[] = []; for (const [a] of [x]) r.push(a); return r; });
out("forof throwing iter", () => { const r: any[] = []; for (const [a] of [{ [Symbol.iterator]() { throw new TypeError("no"); } }] as any[]) r.push(a); return r; });
out("forof typed annotated", () => { const r: number[] = []; const xs: [number, number][] = [[1, 2], [3, 4]]; for (const [a, b] of xs) r.push(a + b); return r; });
out("forof entries", () => { const r: any[] = []; for (const [i, v] of ["a", "b"].entries()) r.push(i, v); return r; });
out("forof map", () => { const r: any[] = []; for (const [k, v] of new Map([["k", 1]])) r.push(k, v); return r; });
out("forof Object.entries", () => { const r: any[] = []; for (const [k, v] of Object.entries({ a: 1 })) r.push(k, v); return r; });
async function aw() { const r: any[] = []; for await (const [p, q] of [mkIt([1, 2]), Promise.resolve(new Set([3, 4]))] as any[]) r.push(p, q); console.log("forawait", JSON.stringify(r), log.join("|")); }
log.length = 0; aw();
