// #5989: `Set`/`Map` `.forEach(callback, thisArg)` must iterate when the
// receiver is a member-access expression (`obj.prop.forEach(...)`), not just a
// plain identifier.
//
// Perry's codegen routes `<expr>.forEach(cb, thisArg)` to the generic
// array-like `forEach` helper when it cannot statically prove the receiver is a
// collection — which is the case for a property-access receiver. That helper
// applied ARRAY semantics (read `.length`, iterate integer indices), but a
// Set/Map has no `.length`, so it read length 0 and silently iterated nothing.
// React's `renderToReadableStream` hit this exactly:
// `renderState.bootstrapScripts.forEach(flushResource, destination)` dropped
// every buffered Float preload-`<link>`, so Next.js force-dynamic pages
// streamed HTML missing their `<link rel="preload">` bootstrap hints.

const out: string[] = [];
function collect(this: { tag: string }, v: string) {
  out.push(this.tag + ":" + v);
}

// Set via a member-access receiver + object thisArg (the React shape).
const state: { names: Set<string> } = { names: new Set(["a", "b", "c"]) };
state.names.forEach(collect, { tag: "S" });
console.log("set member+thisArg:", JSON.stringify(out));

// Map via a member-access receiver + thisArg.
out.length = 0;
const store: { entries: Map<string, number> } = {
  entries: new Map([["x", 1], ["y", 2]]),
};
store.entries.forEach(function (this: { tag: string }, val: number, key: string) {
  out.push(this.tag + ":" + key + "=" + val);
}, { tag: "M" });
console.log("map member+thisArg:", JSON.stringify(out));

// Nested-property receiver (two hops) — the exact writePreamble shape.
out.length = 0;
function writeAll(rs: { res: { boot: Set<string> } }, dest: string[]) {
  rs.res.boot.forEach(function (chunk: string) {
    (this as string[]).push(chunk);
  }, dest);
}
const collected: string[] = [];
writeAll({ res: { boot: new Set(["<link>", "<script>"]) } }, collected);
console.log("nested member forEach:", JSON.stringify(collected));

// forEach with NO thisArg via member receiver must keep working.
out.length = 0;
const holder: { s: Set<number> } = { s: new Set([10, 20]) };
holder.s.forEach((v) => out.push("n:" + v));
console.log("set member no-thisArg:", JSON.stringify(out));

// A genuine array-like reaching the same helper is unaffected.
const arr = [1, 2, 3];
let sum = 0;
arr.forEach(function (this: { base: number }, v: number) {
  sum += v + this.base;
}, { base: 10 });
console.log("array forEach thisArg sum:", sum);
