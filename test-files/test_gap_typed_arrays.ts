// Gap test: TypedArray operations
// Run: node --experimental-strip-types test_gap_typed_arrays.ts

// --- Uint8Array construction ---
const u1 = new Uint8Array(5);
console.log("Uint8Array(5) length:", u1.length);
console.log("Uint8Array(5)[0]:", u1[0]);

const u2 = new Uint8Array([10, 20, 30, 40, 50]);
console.log("from array:", u2[0], u2[1], u2[2], u2[3], u2[4]);
console.log("from array length:", u2.length);

// GetMethod(source, @@iterator) falls back to array-like reads when the
// method is undefined or null.
const iteratorDescriptor = Object.getOwnPropertyDescriptor(
  Array.prototype,
  Symbol.iterator,
)!;
delete (Array.prototype as any)[Symbol.iterator];
const noPrototypeIterator = new Uint8Array([6, 7]);
Object.defineProperty(Array.prototype, Symbol.iterator, iteratorDescriptor);
console.log(
  "from array without prototype iterator:",
  noPrototypeIterator[0],
  noPrototypeIterator[1],
);
const nullIteratorSource: any = [8, 9];
nullIteratorSource[Symbol.iterator] = null;
const nullIterator = new Uint8Array(nullIteratorSource);
console.log("from array with null iterator:", nullIterator[0], nullIterator[1]);

// Array-like indexed Get preserves the source as `this` when an inherited
// accessor supplies the element.
const inheritedIndexSource: any = new Array(1);
inheritedIndexSource[Symbol.iterator] = null;
let inheritedIndexReceiverMatches = false;
let inheritedIndexGetterCalls = 0;
const indexedPrototype: any = [];
Object.defineProperty(indexedPrototype, "0", {
  configurable: true,
  get() {
    inheritedIndexGetterCalls++;
    inheritedIndexReceiverMatches = this === inheritedIndexSource;
    return inheritedIndexReceiverMatches ? 73 : 74;
  },
});
Object.setPrototypeOf(inheritedIndexSource, indexedPrototype);
const inheritedIndexResult = new Int16Array(inheritedIndexSource);
console.log(
  "from inherited indexed getter:",
  inheritedIndexResult[0],
  inheritedIndexReceiverMatches,
  inheritedIndexGetterCalls,
);

// --- Uint8Array read/write ---
const u3 = new Uint8Array(3);
u3[0] = 100;
u3[1] = 200;
u3[2] = 255;
console.log("write:", u3[0], u3[1], u3[2]);

// Overflow wraps to 0-255
u3[0] = 256;
console.log("overflow 256:", u3[0]);
u3[0] = -1;
console.log("negative -1:", u3[0]);

// --- Uint8Array.from ---
const u4 = Uint8Array.from([1, 2, 3, 4]);
console.log("Uint8Array.from:", u4[0], u4[1], u4[2], u4[3]);
console.log("Uint8Array.from length:", u4.length);

// --- Uint8Array iteration ---
const u5 = new Uint8Array([5, 10, 15]);
let sum = 0;
for (let i = 0; i < u5.length; i++) {
  sum += u5[i];
}
console.log("sum:", sum);

// --- Int32Array ---
const i1 = new Int32Array(3);
i1[0] = -100;
i1[1] = 0;
i1[2] = 2147483647;
console.log("Int32Array:", i1[0], i1[1], i1[2]);
console.log("Int32Array length:", i1.length);

// --- Float64Array ---
const f1 = new Float64Array(3);
f1[0] = 3.14;
f1[1] = -2.718;
f1[2] = 0.0;
console.log("Float64Array:", f1[0], f1[1], f1[2]);
console.log("Float64Array length:", f1.length);

// --- Int16Array ---
const i16 = new Int16Array(2);
i16[0] = 32767;
i16[1] = -32768;
console.log("Int16Array:", i16[0], i16[1]);

// --- Uint16Array ---
const u16 = new Uint16Array(2);
u16[0] = 65535;
u16[1] = 0;
console.log("Uint16Array:", u16[0], u16[1]);

// --- Float32Array ---
const f32 = new Float32Array(2);
f32[0] = 1.5;
f32[1] = -0.5;
console.log("Float32Array:", f32[0], f32[1]);

// --- Uint8ClampedArray ---
const clamped = new Uint8ClampedArray(3);
clamped[0] = 300;  // clamped to 255
clamped[1] = -10;  // clamped to 0
clamped[2] = 128;
console.log("Uint8Clamped:", clamped[0], clamped[1], clamped[2]);

// --- TypedArray with variable size ---
const size = 10;
const dynamic = new Uint8Array(size);
console.log("dynamic length:", dynamic.length);

const expr = new Uint8Array(size + 5);
console.log("expr length:", expr.length);

// --- Large TypedArray ---
const large = new Uint8Array(10000);
for (let i = 0; i < 10000; i++) {
  large[i] = i % 256;
}
let largeSum = 0;
for (let i = 0; i < large.length; i++) {
  largeSum += large[i];
}
console.log("large sum:", largeSum);
console.log("large[0]:", large[0]);
console.log("large[255]:", large[255]);
console.log("large[9999]:", large[9999]);
