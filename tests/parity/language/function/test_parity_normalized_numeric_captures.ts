// Captures classified by the callable entry are passed as actual raw values.
function make(n: number): (x: number) => number {
  return function(x: number): number { return (x * 31 + n) % 7; };
}
for (const n of [-4, -0, 0, 2.5, 7 | 0, NaN, Infinity]) {
  const f = make(n);
  for (const x of [-2, -0, 0, 1.5, NaN]) {
    const v = f(x);
    console.log("value", v, 1 / v);
  }
}
const unknown: any = "2";
console.log("string", make(unknown)(1));
let calls = 0;
const object: any = { valueOf() { calls++; return 2; } };
console.log("coerce", make(object)(1), calls);
function mutable(n: number): (x: number) => any {
  let v: any = n;
  const f = function(x: number): any { return x + v; };
  v = "s";
  return f;
}
console.log("mutable", mutable(2)(1));
