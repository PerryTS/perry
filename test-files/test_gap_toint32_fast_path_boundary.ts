// #10897: user-visible ToInt32 takes a guarded hardware conversion when
// |v| < 2^63 and the exact modular tower otherwise (NaN, ±Infinity, huge).
// Every value here reaches the conversion at RUNTIME — array element reads,
// class-field reads, accumulators, typed-array stores — so both arms and the
// boundary between them are exercised in each operator shape that lowers
// through the shared conversion.

const values: number[] = [
  0, -0, 0.5, -0.5, 0.9999999999999999, -0.9999999999999999,
  1, -1, 1.5, -1.5, 123.9, -123.9,
  2147483647, 2147483648, -2147483648, -2147483649,
  4294967295, 4294967296, 4294967297.75, -4294967297.75,
  9007199254740991, -9007199254740991, 9007199254740992,
  // Largest doubles below 2^63: the last values the fast arm accepts.
  9223372036854774784, -9223372036854774784,
  // ±2^63 and beyond: the first values that must take the exact arm.
  9223372036854775808, -9223372036854775808,
  18446744073709551616, 18446744073709555712, -18446744073709555712,
  1e20, -1e20, 3e30, 1.7976931348623157e308, -1.7976931348623157e308,
  5e-324, -5e-324, Infinity, -Infinity, NaN,
];

class Box {
  a: number;
  constructor(a: number) {
    this.a = a;
  }
}

function operators(v: number): string {
  return [v | 0, v ^ 5, v & 0xffff, v << 3, v >> 2, v >>> 0, v >>> 1, ~v].join(" ");
}

for (let i = 0; i < values.length; i++) {
  console.log(String(values[i]), operators(values[i]));
}

// Both operands converted in one expression.
let pairs = "";
for (let i = 0; i + 1 < values.length; i++) {
  const l = values[i];
  const r = values[i + 1];
  pairs += ((l | r) ^ (l & r)) + " ";
}
console.log(pairs);

// The issue's shape: a double accumulator normalised by `| 0` beside a
// class-field read, crossing into and out of the exact arm. Traced per step so
// a wrong conversion is pinned to the value that produced it.
function accumulate(vs: number[]): string {
  let h = 0;
  const trace: number[] = [];
  for (let i = 0; i < vs.length; i++) {
    const o = new Box(vs[i]);
    h = (h + o.a) | 0;
    trace.push(h);
  }
  return trace.join(" ");
}
console.log(accumulate(values));

// A hash-style mixer: products up to ~2^71 land on both arms (and stay below
// 2^84, where every double is a multiple of 2^32 and ToInt32 is trivially 0).
function mix(vs: number[]): string {
  let h = 2166136261 | 0;
  const trace: number[] = [];
  for (let i = 0; i < vs.length; i++) {
    h = ((h ^ (vs[i] | 0)) * 16777619 * 65599) | 0;
    trace.push(h >>> 0);
  }
  return trace.join(" ");
}
console.log(mix(values));

// Integer typed-array stores perform ToInt32/ToUint32 of the stored value.
const i32 = new Int32Array(values.length);
const u32 = new Uint32Array(values.length);
const i16 = new Int16Array(values.length);
const u8 = new Uint8Array(values.length);
for (let i = 0; i < values.length; i++) {
  i32[i] = values[i];
  u32[i] = values[i];
  i16[i] = values[i];
  u8[i] = values[i];
}
console.log(Array.from(i32).join(" "));
console.log(Array.from(u32).join(" "));
console.log(Array.from(i16).join(" "));
console.log(Array.from(u8).join(" "));

// Node Buffer byte stores take the same conversion.
const buf = Buffer.alloc(values.length);
for (let i = 0; i < values.length; i++) {
  buf[i] = values[i];
}
console.log(Array.from(buf).join(" "));
