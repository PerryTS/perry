// #5016 regression guard: writes to module-level array/object members performed
// inside a function (and at top level) must persist on the web/wasm target.
// Before the fix, `A[0] = ...` lowered to a `PutValueSet` HIR node that the WASM
// codegen never handled, so the write fell through to the undefined catch-all and
// reads returned the initializer. Native always worked; this guards the web path.

const A: number[] = [0.0];
function bump(n: number): void {
  A[0] = A[0] + n;
} // write a module-level array element inside a function
function readA(): number {
  return A[0];
}
const step: number = Date.now() > 0.0 ? 5.0 : 1.0; // runtime value → not const-folded
bump(step);
console.log("fnRead=" + readA().toString());
console.log("topRead=" + A[0].toString());

// Top-level element write must persist too.
A[0] = 99.0;
console.log("afterTop=" + A[0].toString());

// Write through a parameter aliasing the module array.
function setVia(arr: number[], v: number): void {
  arr[0] = v;
}
setVia(A, 7.0);
console.log("viaParam=" + A[0].toString());

// Object property write (string key) inside a function.
const o: { x: number } = { x: 1.0 };
function bumpObj(): void {
  o.x = o.x + 10.0;
}
bumpObj();
console.log("objProp=" + o.x.toString());
