// A JS value crossing into a wasm i32 goes through ToNumber, then ToInt32
// (modulo 2^32); into an f64 through ToNumber. Covers import results,
// exported-function arguments and WebAssembly.Global values.
// Module A: (import "env" "f" (func $f (result i32))) (func (export "g") (result i32) call $f)
const A = new Uint8Array([
  0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
  0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f,
  0x02, 0x09, 0x01, 0x03, 0x65, 0x6e, 0x76, 0x01, 0x66, 0x00, 0x00,
  0x03, 0x02, 0x01, 0x00,
  0x07, 0x05, 0x01, 0x01, 0x67, 0x00, 0x01,
  0x0a, 0x06, 0x01, 0x04, 0x00, 0x10, 0x00, 0x0b,
]);
// Module B: (func (export "id") (param i32) (result i32) local.get 0)
const B = new Uint8Array([
  0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
  0x01, 0x06, 0x01, 0x60, 0x01, 0x7f, 0x01, 0x7f,
  0x03, 0x02, 0x01, 0x00,
  0x07, 0x06, 0x01, 0x02, 0x69, 0x64, 0x00, 0x00,
  0x0a, 0x06, 0x01, 0x04, 0x00, 0x20, 0x00, 0x0b,
]);
const boxed = { valueOf() { return 42; } };
const values: any[] = [true, false, 1, 2.5, -2.5, -1, 4294967295, 2147483648, 1e20, "7", " 12 ", "x", null, undefined, boxed];
let ret: any = 0;
const g: any = new WebAssembly.Instance(new WebAssembly.Module(A), { env: { f: () => ret } }).exports.g;
const id: any = new WebAssembly.Instance(new WebAssembly.Module(B), {}).exports.id;
const i32 = new WebAssembly.Global({ value: "i32", mutable: true }, 0);
const f64 = new WebAssembly.Global({ value: "f64", mutable: true }, 0);
for (const v of values) {
  ret = v;
  i32.value = v;
  f64.value = v;
  console.log(
    String(v).padEnd(12),
    "import", String(g()).padEnd(11),
    "arg", String(id(v)).padEnd(11),
    "global", String(i32.value).padEnd(11),
    "f64", f64.value,
    "init", new WebAssembly.Global({ value: "i32" }, v).value,
  );
}
console.log(new WebAssembly.Global({ value: "f64" }).value, new WebAssembly.Global({ value: "i32" }, undefined).value);
