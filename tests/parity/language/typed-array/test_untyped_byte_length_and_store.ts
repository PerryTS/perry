// #10515: `.length` on an untyped receiver reads a typed array's or byte
// view's header, and a `Uint8Array` store of a value that is not provably a
// Number stores a runtime Number inline. Every receiver and value the inline
// arms refuse must keep ordinary semantics.

function len(x: any): any {
  return x.length;
}

function sumAll(b: any): number {
  let s = 0;
  for (let i = 0; i < b.length; i++) s += b[i];
  return s;
}

const u8 = new Uint8Array([1, 2, 3, 4, 5]);
const buf = Buffer.from([9, 8, 7]);
const i32 = new Int32Array(6);
const f64 = new Float64Array(2);
const view = u8.subarray(1, 4);
const dv = new DataView(new ArrayBuffer(8));
console.log(len(u8), len(buf), len(i32), len(f64), len(view), len(dv));
console.log(len([1, 2]), len("abc"), len({ length: "n" }), len(function (a: any, b: any) {}));
console.log(sumAll(u8), sumAll(buf), sumAll(view));

// A length-tracking view follows its resizable buffer.
const rab = new ArrayBuffer(4, { maxByteLength: 16 });
const tracking = new Uint8Array(rab);
console.log(len(tracking));
rab.resize(12);
console.log(len(tracking));

// A detached buffer's view has length 0.
const owner = new ArrayBuffer(8);
const onDetached = new Uint8Array(owner);
console.log(len(onDetached));
owner.transfer();
console.log(len(onDetached));

// A Proxy over a typed array runs its trap.
const p = new Proxy(new Uint8Array(3), {
  get(t: any, k: any) {
    return k === "length" ? 42 : t[k];
  },
});
console.log(len(p));

// An own `length` shadows the intrinsic accessor.
const own = new Uint8Array(4);
Object.defineProperty(own, "length", { value: 99 });
console.log(len(own), len(new Uint8Array(4)));

// A subclass getter shadows the intrinsic accessor.
class Sized extends Uint8Array {
  get length(): number {
    return 7;
  }
}
console.log(len(new Sized(2)));

// A Uint8Array-annotated store of an unproven value.
function put(b: Uint8Array, src: any, i: number): any {
  return (b[i] = src[i]);
}
const target = new Uint8Array(10);
const values: any[] = [undefined, "5", { valueOf: () => 7 }, -1, 300, 1.5, NaN, null, true, 2 ** 40];
for (let i = 0; i < values.length; i++) console.log(String(put(target, values, i)));
console.log(Array.from(target).join(","));
console.log(String(put(target, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12], 11)), target.length);
const btarget = Buffer.alloc(3);
console.log(String(put(btarget as any, [3, "4", undefined], 0)), String(put(btarget as any, [3, "4", undefined], 1)));
console.log(Array.from(btarget).join(","));
const frozenLike = new Uint8Array(0);
console.log(String(put(frozenLike, [5], 0)), frozenLike.length);

// A redefined prototype accessor withdraws the header answer. (Not on
// %TypedArray%.prototype: Node's own stdout writes read that `length`.)
Object.defineProperty(Int32Array.prototype, "length", {
  get() {
    return -1;
  },
  configurable: true,
});
console.log(len(i32), len(new Int32Array(3)), len(u8));
