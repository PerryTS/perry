// `.length` on statically typed typed arrays: the inline header read must agree
// with the full property lookup for views, own properties and subclasses.

function f64Len(a: Float64Array): number {
  return a.length;
}
function i8Len(a: Int8Array): number {
  return a.length;
}
function u8Len(a: Uint8Array): number {
  return a.length;
}
function anyLen(a: any): any {
  return a.length;
}

const plain = new Float64Array(3);
console.log("plain", f64Len(plain), anyLen(plain));

let total = 0;
const loop = new Float64Array(64);
for (let i = 0; i < 1000; i++) {
  total += f64Len(loop);
}
console.log("loop", total);

const kinds = [
  new Int8Array(1),
  new Uint8Array(2),
  new Uint8ClampedArray(3),
  new Int16Array(4),
  new Uint16Array(5),
  new Int32Array(6),
  new Uint32Array(7),
  new Float32Array(8),
  new Float64Array(9),
  new BigInt64Array(10),
  new BigUint64Array(11),
];
console.log("kinds", kinds.map((k) => (k as any).length).join(","));
console.log("int8", i8Len(kinds[0] as Int8Array), "u8", u8Len(kinds[1] as Uint8Array));

const u8 = new Uint8Array([1, 2, 3, 4, 5, 6]);
console.log("subarray", u8Len(u8.subarray(2)), u8Len(u8.slice(1, 3)));

class Sub extends Float64Array {}
const sub = new Sub(4);
console.log("subclass", f64Len(sub), anyLen(sub));

const buf = new ArrayBuffer(32);
const view = new Float64Array(buf, 8, 2);
console.log("view", f64Len(view), anyLen(view), f64Len(plain));

// A lying annotation.
console.log("array as typed", f64Len([1, 2, 3, 4, 5] as any), f64Len("abc" as any));

// An own `length` property shadows the prototype getter, even for an array
// that was read before the property was defined.
const shadowed = new Float64Array(5);
console.log("before own prop", f64Len(shadowed));
Object.defineProperty(shadowed, "length", { value: 77 });
console.log("own data length", f64Len(shadowed), anyLen(shadowed), f64Len(plain));
const other = new Float64Array(6);
console.log("unrelated after own prop", f64Len(other));
