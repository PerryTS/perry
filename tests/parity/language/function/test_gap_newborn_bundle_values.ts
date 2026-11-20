// A function using `arguments` or a rest parameter receives a fresh array of its arguments
// (the entry bundle) and array literals of evaluated values (js_array_from_values): both are
// newborns whose layout and barrier are published once after the element copy. Heap values
// survive collections between calls and after the bundle is stored somewhere old; numbers stay
// numbers. Compared against Node.
const keep: any[] = [];
function viaArguments() { keep.push(arguments); return arguments.length; }
function viaRest(...xs: any[]) { keep.push(xs); return xs.length; }
function forwardApply(this: any) { return viaRest.apply(this, arguments as any); }
let n = 0;
for (let i = 0; i < 30000; i++) {
  const o = { i, s: "v" + i, a: [i, i + 1] };
  n += viaArguments(o, i, "str" + (i & 15), o.a, 1.5);
  n += viaRest(i * 0.5, o, null, undefined, true);
  n += (forwardApply as any)(o.s, o);
  if (keep.length > 3000) keep.splice(0, 2000);
  // Allocation pressure between calls.
  const junk = new Array(16).fill(0).map((_, k) => ({ k, t: "j" + k }));
  n += junk.length & 1;
}
let check = 0;
for (const b of keep) {
  for (let k = 0; k < b.length; k++) {
    const v = b[k];
    check += typeof v === "object" && v !== null ? (v.i ?? v.length ?? 1) : typeof v === "number" ? v : String(v).length;
  }
}
console.log(n, keep.length, check);
const lit = (a: any, b: any) => [a, b, { a }, [b], "x" + a];
let s = "";
for (let i = 0; i < 2000; i++) { const l = lit(i, { i }); s = JSON.stringify(l); }
console.log(s);
