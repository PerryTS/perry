// Negative control of test_new_promise_only_program.ts: constructors that
// schedule nothing (built-in and declared) in a program with nothing
// asynchronous. It needs no event loop and prints the same with one.
class Stack {
  items: number[] = [];
  push(x: number): number {
    return this.items.push(x);
  }
}
class Tall extends Stack {
  height(): number {
    return this.items.length;
  }
}
const tall = new Tall();
tall.push(3);
tall.push(4);
const shared = new SharedArrayBuffer(16);
const set = new Set([1, 2, 2]);
const error = new RangeError("range");
const bytes = new Uint8Array([1, 2, 3]);
const floats = new Float64Array(2);
const buffer = new ArrayBuffer(8);
const view = new DataView(buffer);
const date = new Date(0);
const regex = new RegExp("a+", "g");
const weak = new WeakMap<object, number>();
weak.set(tall, 1);
const url = new URL("http://example.test/path?q=1");
const params = new URLSearchParams("a=1&b=2");
const encoded = new TextEncoder().encode("hé");
const decoded = new TextDecoder().decode(encoded);
const proxy: any = new Proxy(
  { x: 1 },
  {
    get(target: any, key: any) {
      return key in target ? target[key] * 2 : key;
    },
  },
);
const ref = new WeakRef(tall);
const boxed = new Number(5);
console.log(
  tall.height(),
  shared.byteLength,
  set.size,
  error.name,
  bytes.length,
  floats.length,
  view.byteLength,
  date.getTime(),
  "caaat".replace(regex, "-"),
  weak.get(tall),
  url.pathname,
  params.get("b"),
  encoded.length,
  decoded,
  proxy.x,
  proxy.other,
  ref.deref() === tall,
  +boxed + 1,
);
