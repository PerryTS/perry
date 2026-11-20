// Stress test: Integer optimization correctness
// Targets: i32 fast path, accumulator patterns, loop counters + closures,
// bitwise operations, overflow behavior
// Based on bugs: v0.5.39 boxed_vars, v0.5.40-43 int-arithmetic fast path

// === SECTION: Basic integer accumulator ===
let acc = 0;
for (let i = 0; i < 1000; i++) {
  acc += i;
}
console.log("sum 0..999:", acc);

// === SECTION: Accumulator with multiplication ===
let product = 1;
for (let i = 1; i <= 20; i++) {
  product = (product * i) | 0;
}
console.log("20! (i32):", product);

// === SECTION: Accumulator with subtraction ===
let diff = 10000;
for (let i = 0; i < 100; i++) {
  diff -= i;
}
console.log("10000 - sum(0..99):", diff);

// === SECTION: Bitwise OR zero pattern ===
console.log("(3.7) | 0:", (3.7) | 0);
console.log("(-3.7) | 0:", (-3.7) | 0);
console.log("(NaN) | 0:", (NaN) | 0);
console.log("(Infinity) | 0:", (Infinity) | 0);
console.log("(-Infinity) | 0:", (-Infinity) | 0);
console.log("(2147483647) | 0:", (2147483647) | 0);
console.log("(2147483648) | 0:", (2147483648) | 0);
console.log("(-2147483648) | 0:", (-2147483648) | 0);
console.log("(-2147483649) | 0:", (-2147483649) | 0);

// === SECTION: Unsigned right shift zero pattern ===
console.log("(-1) >>> 0:", (-1) >>> 0);
console.log("(0) >>> 0:", (0) >>> 0);
console.log("(4294967295) >>> 0:", (4294967295) >>> 0);
console.log("(4294967296) >>> 0:", (4294967296) >>> 0);
console.log("(-1.5) >>> 0:", (-1.5) >>> 0);

// === SECTION: Bitwise operations on integers ===
console.log("0xFF & 0x0F:", 0xFF & 0x0F);
console.log("0xF0 | 0x0F:", 0xF0 | 0x0F);
console.log("0xFF ^ 0x0F:", 0xFF ^ 0x0F);
console.log("~0:", ~0);
console.log("~(-1):", ~(-1));
console.log("1 << 31:", 1 << 31);
console.log("-1 >> 1:", -1 >> 1);
console.log("-1 >>> 1:", -1 >>> 1);

// === SECTION: Loop counter with closure (v0.5.39 boxed_vars bug) ===
// This pattern previously caused the loop counter to be box-allocated
const results: number[] = [];
for (let i = 0; i < 10; i++) {
  results.push(i * i);
}
console.log("squares:", results);

// Same pattern but counter used after loop
let counter = 0;
for (let j = 0; j < 100; j++) {
  counter = (counter + j) | 0;
}
console.log("counter after loop:", counter);

// === SECTION: Nested integer loops ===
let total = 0;
for (let y = 0; y < 50; y++) {
  for (let x = 0; x < 50; x++) {
    total += x * y;
  }
}
console.log("nested loop total:", total);

// === SECTION: Accumulator with array element reads ===
const data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
let arrSum = 0;
for (let i = 0; i < data.length; i++) {
  arrSum += data[i];
}
console.log("array sum:", arrSum);

// === SECTION: Accumulator with 2D array lookup ===
const matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
let matSum = 0;
for (let r = 0; r < 3; r++) {
  for (let c = 0; c < 3; c++) {
    matSum += matrix[r][c];
  }
}
console.log("matrix sum:", matSum);

// === SECTION: Integer edge values in arithmetic ===
console.log("MAX_SAFE + 1:", Number.MAX_SAFE_INTEGER + 1);
console.log("MAX_SAFE + 2:", Number.MAX_SAFE_INTEGER + 2);
console.log("MIN_SAFE - 1:", Number.MIN_SAFE_INTEGER - 1);

// i32 boundaries
const i32Max = 2147483647;
const i32Min = -2147483648;
console.log("i32Max:", i32Max);
console.log("i32Max + 1:", i32Max + 1);
console.log("i32Min:", i32Min);
console.log("i32Min - 1:", i32Min - 1);

// === SECTION: Mixed int/float in same loop ===
let intAcc = 0;
let floatAcc = 0.0;
for (let i = 0; i < 100; i++) {
  intAcc = (intAcc + i) | 0;
  floatAcc += i * 0.5;
}
console.log("intAcc:", intAcc);
console.log("floatAcc:", floatAcc);

// === SECTION: Byte accumulator (common in Buffer/image code) ===
const bytes = new Uint8Array(256);
for (let i = 0; i < 256; i++) {
  bytes[i] = i;
}
let byteSum = 0;
for (let i = 0; i < bytes.length; i++) {
  byteSum += bytes[i];
}
console.log("byte sum 0..255:", byteSum);

// === SECTION: Weighted accumulator (image convolution pattern) ===
const kernel = [[-1, -1, -1], [-1, 8, -1], [-1, -1, -1]];
const pixels = [100, 150, 200, 120, 180, 160, 130, 170, 190];
let convResult = 0;
for (let ky = 0; ky < 3; ky++) {
  for (let kx = 0; kx < 3; kx++) {
    convResult += pixels[ky * 3 + kx] * kernel[ky][kx];
  }
}
console.log("convolution result:", convResult);

// === SECTION: Math.imul edge cases ===
console.log("imul(2, 3):", Math.imul(2, 3));
console.log("imul(-1, 8):", Math.imul(-1, 8));
console.log("imul(0xFFFFFFFF, 5):", Math.imul(0xFFFFFFFF, 5));
console.log("imul(0x7FFFFFFF, 0x7FFFFFFF):", Math.imul(0x7FFFFFFF, 0x7FFFFFFF));
