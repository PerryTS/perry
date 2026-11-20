// Issue #5504 — calling a non-callable value must throw a TypeError, not
// SIGSEGV. The old call-emission path masked the callee's low 48 bits and
// handed them to `js_closure_callN` as a closure pointer without checking the
// NaN-box tag. A non-callable NUMBER whose mantissa's low 48 bits happen to
// form an in-range address (e.g. `1e-8` → 0x798E_E230_8C3A) passed the
// runtime's range check and faulted on the closure-header read.
//
// We compare byte-for-byte against `node --experimental-strip-types`, so we
// normalize on `instanceof TypeError` rather than the engine-specific error
// text (Node: "g is not a function" vs Perry: "value is not a function").

function callThrows(label: string, f: () => void): void {
  try {
    f();
    console.log(label + " -> NO THROW");
  } catch (e) {
    console.log(label + " -> " + (e instanceof TypeError ? "TypeError" : String(e)));
  }
}

// Numbers whose low-48 mantissa bits form an in-range "pointer" (the crashing
// cases) and ones that mask to zero (already threw). All must throw TypeError.
callThrows("1e-8()", () => {
  const g: any = (() => 1e-8)();
  g(1);
});
callThrows("Math.PI()", () => {
  const g: any = (() => Math.PI)();
  g();
});
callThrows("0xf0001-scaled()", () => {
  const g: any = (() => 0xf0001 * 1e-300)();
  g();
});
callThrows("5()", () => {
  const g: any = (() => 5)();
  g(1, 2);
});
callThrows("2.5()", () => {
  const g: any = (() => 2.5)();
  g();
});
// >16 args routes through the `js_closure_call_array` branch, which was also
// switched to the checked unbox — exercise it with a crashing-shaped number.
callThrows("1e-8(17 args)", () => {
  const g: any = (() => 1e-8)();
  g(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17);
});

// Other non-callable primitives.
callThrows('"hi"()', () => {
  const g: any = (() => "hi")();
  g();
});
callThrows("true()", () => {
  const g: any = (() => true)();
  g();
});
callThrows("undefined()", () => {
  const g: any = (() => undefined)();
  g();
});
callThrows("null()", () => {
  const g: any = (() => null)();
  g();
});

// Valid callables (dynamically typed) must still dispatch correctly.
const add: any = (a: number, b: number) => a + b;
console.log("add(40, 2) =", add(40, 2));

const noArg: any = () => "ok";
console.log("noArg() =", noArg());

class Counter {
  v = 7;
  get(): number {
    return this.v;
  }
}
const c = new Counter();
const bound: any = c.get.bind(c);
console.log("bound() =", bound());

console.log("survived all calls");
