// Total numeric comparisons may be scheduled; coercions and calls may not.
function numeric(limit: number): number {
  let x = 0.25;
  let count = 0;
  while (x * x <= 4 && count < limit) { x = x * 1.125 + 0.1; count++; }
  return count;
}
for (const limit of [-1, -0, 0, 1, 2.5, 8, NaN, Infinity]) {
  console.log("numeric", numeric(limit));
}
let trace = "";
const a: any = { valueOf() { trace += "a"; return 3; } };
let count = 0;
while (a * a < 4 && count < 0) { count++; }
console.log("coercion", trace, count);
function first(): boolean { trace += "L"; return false; }
function second(): boolean { trace += "R"; return true; }
console.log("effects", first() && second(), trace);
let captured: any = 3;
function mutate(): boolean { captured = 1; return true; }
while (captured * captured < 4 && mutate()) { break; }
console.log("capture", captured);
let n: any = "1";
let k = 0;
while (n * n < 4 && k < 1) { n = n + 1; k++; }
console.log("string", n, k);
