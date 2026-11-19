// Math.imul through the runtime helper (operands the compiler cannot prove
// int32: values read from an `any` array, strings, objects): ToInt32 of
// every magnitude class, below 2^31, below 2^53, below 2^63 and beyond it,
// fractions, negative zero and the non-finite values, plus the conversions
// a non-number operand takes first (valueOf runs once per operand).

const vals: any[] = [
  0, -0, 1, -1, 1.9, -1.9, 2 ** 31 - 1, 2 ** 31, -(2 ** 31), -(2 ** 31) - 1,
  2 ** 32 - 1, 2 ** 32, 2 ** 32 + 3, -(2 ** 32) - 3, 2 ** 52 + 1, 2 ** 53, -(2 ** 53) + 7,
  2 ** 62 + 2 ** 40, 2 ** 63 - 2 ** 11, 2 ** 63, -(2 ** 63), 2 ** 64 + 2 ** 12, 1e20, -1e20,
  1.7976931348623157e308, 5e-324, -5e-324, NaN, Infinity, -Infinity, 0x9e3779b1, 0x85ebca6b,
];
const out: number[] = [];
for (let i = 0; i < vals.length; i++) {
  for (let j = 0; j < vals.length; j += 3) out.push(Math.imul(vals[i], vals[j]));
}
console.log(out.join(","));

let order = "";
const a = { valueOf() { order += "a"; return 2 ** 32 + 7; } };
const b = { valueOf() { order += "b"; return -3; } };
console.log(Math.imul(a as any, b as any), order);
console.log(Math.imul("12" as any, "-4" as any), Math.imul("0x10" as any, 3), Math.imul("x" as any, 3));
console.log(Math.imul(true as any, 7), Math.imul(null as any, 7), Math.imul(undefined as any, 7));
console.log(Object.is(Math.imul(-0, 5), 0), Object.is(Math.imul(-1, 0), 0));
try {
  Math.imul(1n as any, 2);
} catch (e: any) {
  console.log(e.constructor.name);
}
try {
  Math.imul(Symbol() as any, 2);
} catch (e: any) {
  console.log(e.constructor.name);
}
// FNV-1a style hashing (the shape of the app's recorder) stays exact.
let h = 0x811c9dc5 | 0;
for (let i = 0; i < 1000; i++) h = Math.imul(h ^ (i & 255), 16777619);
console.log(h >>> 0);
const imul: any = Math.imul;
console.log(imul(2 ** 40 + 5, 3), [1e21, 7].map((x) => imul(x, 3)).join(","));
