// Gap test for #2804 / #2801 / #2800 / #2802:
// Array prototype optional-argument semantics — indexOf/includes fromIndex,
// fill(value,start,end), flat(depth), copyWithin(target,start,end).
// Each method is exercised via BOTH a direct call and a dynamic-dispatch call
// (computed method name or `as any` receiver) so both codegen paths are tested.

// ---------- #2804: indexOf / includes fromIndex ----------
const arr = [1, 2, 1, NaN];
const ta = new Float64Array([1, 2, 1, NaN]);

// Direct calls.
console.log("indexOf from 1:", arr.indexOf(1, 1));
console.log("indexOf from -2:", arr.indexOf(1, -2));
console.log("indexOf from Infinity:", arr.indexOf(1, Infinity));
console.log("indexOf default:", arr.indexOf(1));
console.log("includes NaN:", arr.includes(NaN));
console.log("includes from 4:", arr.includes(1, 4));
console.log("includes from -2:", arr.includes(1, -2));
console.log("includes from Infinity:", arr.includes(1, Infinity));

console.log("ta indexOf from 1:", ta.indexOf(1, 1));
console.log("ta indexOf from -2:", ta.indexOf(1, -2));
console.log("ta indexOf from Infinity:", ta.indexOf(1, Infinity));
console.log("ta includes NaN:", ta.includes(NaN));
console.log("ta includes from 4:", ta.includes(1, 4));

// Dynamic dispatch (computed method name).
const mi = "indexOf";
const mc = "includes";
console.log("dyn indexOf from 1:", (arr as any)[mi](1, 1));
console.log("dyn indexOf from -2:", (arr as any)[mi](1, -2));
console.log("dyn indexOf from Infinity:", (arr as any)[mi](1, Infinity));
console.log("dyn includes from 4:", (arr as any)[mc](1, 4));
console.log("dyn includes from -2:", (arr as any)[mc](1, -2));
console.log("dyn includes NaN:", (arr as any)[mc](NaN));

// ---------- #2801: fill(value, start, end) ----------
console.log("fill value only:", JSON.stringify([1, 2, 3, 4].fill(9)));
console.log("fill start:", JSON.stringify([1, 2, 3, 4].fill(9, 2)));
console.log("fill start end:", JSON.stringify([1, 2, 3, 4].fill(9, 1, 3)));
console.log("fill neg:", JSON.stringify([1, 2, 3, 4].fill(9, -3, -1)));

const f1 = [1, 2, 3, 4];
const f2 = [1, 2, 3, 4];
const f3 = [1, 2, 3, 4];
console.log("dyn fill start:", JSON.stringify((f1 as any).fill(9, 2)));
console.log("dyn fill start end:", JSON.stringify((f2 as any).fill(9, 1, 3)));
console.log("dyn fill neg:", JSON.stringify((f3 as any).fill(9, -3, -1)));

// ---------- #2800: flat(depth) ----------
function nest(depth: number): any {
  let x: any = "leaf";
  for (let i = 0; i < depth; i++) x = [x];
  return x;
}
function residualDepth(out: any): number {
  let d = 0;
  let cur = out;
  while (Array.isArray(cur)) {
    d++;
    cur = cur[0];
    if (d > 50) break;
  }
  return d;
}
for (const depth of [0, 1, 2, 32, 40, Infinity, NaN, -1]) {
  console.log(`flat ${String(depth)}:`, residualDepth(nest(40).flat(depth)));
}
// Dynamic dispatch flat.
const fm = "flat";
console.log("dyn flat 0:", residualDepth((nest(40) as any)[fm](0)));
console.log("dyn flat 2:", residualDepth((nest(40) as any)[fm](2)));
console.log("dyn flat Infinity:", residualDepth((nest(40) as any)[fm](Infinity)));
console.log("dyn flat default:", residualDepth((nest(40) as any)[fm]()));

// ---------- #2802: copyWithin(target, start, end) ----------
console.log("copyWithin target:", JSON.stringify([1, 2, 3, 4].copyWithin(1)));
console.log("copyWithin t start:", JSON.stringify([1, 2, 3, 4].copyWithin(1, 2)));
console.log("copyWithin t s e:", JSON.stringify([1, 2, 3, 4].copyWithin(1, 0, 2)));
console.log("copyWithin neg:", JSON.stringify([1, 2, 3, 4].copyWithin(-2, 0, -1)));

const c1 = [1, 2, 3, 4];
const c2 = [1, 2, 3, 4];
const c3 = [1, 2, 3, 4];
const c4 = [1, 2, 3, 4];
console.log("dyn copyWithin target:", JSON.stringify((c1 as any).copyWithin(1)));
console.log("dyn copyWithin t start:", JSON.stringify((c2 as any).copyWithin(1, 2)));
console.log("dyn copyWithin t s e:", JSON.stringify((c3 as any).copyWithin(1, 0, 2)));
console.log("dyn copyWithin neg:", JSON.stringify((c4 as any).copyWithin(-2, 0, -1)));
