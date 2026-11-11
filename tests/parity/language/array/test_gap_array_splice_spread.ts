// #6870: `splice`/`unshift` on a typed local array dropped the spread flag and
// inserted the spread source as ONE nested element instead of expanding it.
// The typed-local fast path in local_array_methods.rs materialized each item as
// a single ArraySplice/ArrayUnshift element; `push`/`concat` were unaffected.
// Spread calls now fall through to generic dispatch, which spreads correctly.

const src: string[] = ["X", "Y"];

// --- splice: spread items -------------------------------------------------
const a: string[] = ["a", "b"];
a.splice(1, 0, ...src);
console.log(JSON.stringify(a));

// spread of an array literal
const b: string[] = ["a", "b"];
b.splice(1, 0, ...["P", "Q"]);
console.log(JSON.stringify(b));

// spread mixed with explicit args, before and after
const c: string[] = ["a", "b"];
c.splice(1, 0, "L", ...src, "R");
console.log(JSON.stringify(c));

// spread combined with a non-zero deleteCount
const d: string[] = ["a", "b", "c"];
const dDeleted = d.splice(1, 2, ...src);
console.log(JSON.stringify(d), JSON.stringify(dDeleted));

// spreading an empty array deletes without inserting
const e: string[] = ["a", "b"];
const eEmpty: string[] = [];
e.splice(1, 1, ...eEmpty);
console.log(JSON.stringify(e));

// numeric elements, spread at the end
const f: number[] = [1, 4];
f.splice(1, 0, ...[2, 3]);
console.log(JSON.stringify(f));

// --- unshift: spread items ------------------------------------------------
const g: string[] = ["a", "b"];
g.unshift(...src);
console.log(JSON.stringify(g));

const h: string[] = ["a"];
h.unshift(...["P", "Q"], "R");
console.log(JSON.stringify(h));

const i: string[] = ["a"];
const iEmpty: string[] = [];
console.log(i.unshift(...iEmpty), JSON.stringify(i));

// --- non-spread forms must keep taking the fast path ----------------------
const j: string[] = ["a", "b"];
j.splice(1, 0, "P", "Q");
console.log(JSON.stringify(j));

const k: string[] = ["a", "b"];
k.splice(1, 1);
console.log(JSON.stringify(k));

const l: string[] = ["a", "b"];
l.unshift("P");
console.log(JSON.stringify(l));

// return value is the deleted elements, spread or not
const m: number[] = [1, 2, 3, 4];
console.log(JSON.stringify(m.splice(1, 2, ...[9])), JSON.stringify(m));

// --- push/concat stayed correct throughout --------------------------------
const n: string[] = ["a"];
n.push(...src);
console.log(JSON.stringify(n));
console.log(JSON.stringify((["a"] as string[]).concat(src)));
