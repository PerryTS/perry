// Issue #311: for...of on Map/Set as object property silently iterated zero
// times. Pre-fix the resolver only handled `Ident` (local) and
// `Member { obj: This }` (class field per #302); `obj.<prop>` fell through
// to None, the loop read .length on a raw Map handle (returned 0) and
// produced no iterations.

// Case 1: Map as object property — shorthand
const m = new Map<number, string>();
m.set(1, "hello");
m.set(2, "world");
const obj = { m };
console.log("case1:");
for (const [k, v] of obj.m) {
  console.log(k, v);
}

// Case 2: Set as object property — shorthand
const s = new Set<number>([42, 99]);
const obj2 = { s };
console.log("case2:");
for (const v of obj2.s) {
  console.log(v);
}

// Case 3: Array as object property — shorthand
const xs = [10, 20, 30];
const obj3 = { xs };
console.log("case3:");
let sum = 0;
for (const x of obj3.xs) {
  sum += x;
}
console.log("sum:", sum);

// Case 4: KeyValue (not shorthand) — `{ items: m }` with explicit key
const m2 = new Map<string, number>();
m2.set("a", 1);
m2.set("b", 2);
const wrapper = { items: m2 };
console.log("case4:");
for (const [k, v] of wrapper.items) {
  console.log(k, v);
}

// Case 5: nested wrapper — different name on the property
const inner = new Set<string>(["x", "y"]);
const outer = { data: inner };
console.log("case5:");
for (const v of outer.data) {
  console.log(v);
}

// Case 6: pre-existing local-Map case still works (regression guard)
console.log("case6:");
for (const [k, v] of m) {
  console.log(k, v);
}

// Case 7: class instance Map field — receiver `obj` typed as `Example`
class Example {
  m: Map<number, string> = new Map([[1, "hello"], [2, "world"]]);
  s: Set<number> = new Set([10, 20]);
  xs: number[] = [100, 200, 300];
}
const ex = new Example();
console.log("case7-map:");
for (const [k, v] of ex.m) {
  console.log(k, v);
}
console.log("case7-set:");
for (const v of ex.s) {
  console.log(v);
}
console.log("case7-array:");
for (const x of ex.xs) {
  console.log(x);
}
