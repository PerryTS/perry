// `v instanceof <TypedArray constructor>` at one site, before and after the
// constructor's chain gains an @@hasInstance (own and inherited).
function check(v: any, C: any): boolean {
  return v instanceof C;
}
const vals: any[] = [
  new Float32Array(2),
  new Float32Array(new ArrayBuffer(16), 4, 2),
  new Int8Array(2),
  new Uint8Array(2),
  [1],
  {},
  1,
  "s",
  null,
  undefined,
];
const ctors: any[] = [Float32Array, Int8Array, Uint8Array, Float64Array];
const row = () => ctors.map((C) => C.name + ":" + vals.map((v) => (check(v, C) ? 1 : 0)).join("")).join(" ");
console.log("default", row());
class Sub extends Int8Array {}
console.log("sub", check(new Sub(1), Int8Array), check(new Sub(1), Float32Array), check(new Sub(1), Sub));
const relinked = new Float32Array(2);
Object.setPrototypeOf(relinked, Int8Array.prototype);
console.log("relinked value", check(relinked, Float32Array), check(relinked, Int8Array));
Object.defineProperty(Object.getPrototypeOf(Float32Array), Symbol.hasInstance, { value: (v: unknown) => v === 1 });
console.log("parent hook", row());
Object.defineProperty(Uint8Array, Symbol.hasInstance, { value: () => false });
console.log("own hook", row());
