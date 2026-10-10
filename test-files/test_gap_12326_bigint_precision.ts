// #12326: Effect 4.0.2's arbitrary generator initializes these values.
const maximum = BigInt(Number.MAX_VALUE);
const next = maximum + 1n;
const above = 1n << 1024n;
console.log("max", maximum.toString(16).length, maximum.toString(16).slice(0, 13));
console.log("next", next - maximum === 1n, next > maximum, next > Number.MAX_VALUE);
console.log("shift", above.toString(16).length, above.toString(16).slice(0, 2), above > maximum);
console.log("number", Number(maximum) === Number.MAX_VALUE, Number(above) === Infinity);
console.log("compare", maximum == Number.MAX_VALUE, maximum < next, -above < -maximum);

// Constructors, arithmetic and bitwise operations share one precision.
const wide = (1n << 4096n) + (1n << 2048n) + 123n;
const parsed = BigInt("0x" + wide.toString(16));
const negative = -wide;
console.log("parse", parsed === wide, BigInt(wide.toString()) === wide);
console.log("signed", -negative === wide, negative + wide === 0n, ~wide === -wide - 1n);
console.log("multiply", (wide * wide) / wide === wide, (wide * wide) % wide === 0n);
console.log("division", negative / 7n === -(wide / 7n), negative % 7n === -(wide % 7n));
console.log("power", 2n ** 4096n === 1n << 4096n, (-2n) ** 4097n === -(1n << 4097n));
console.log("bitwise", (wide & 255n) === 123n, (negative >> 4096n) === -2n, (wide ^ parsed) === 0n);
console.log("wrap", BigInt.asUintN(4097, -1n) === (1n << 4097n) - 1n,
  BigInt.asIntN(4097, 1n << 4096n) === -(1n << 4096n), BigInt.asUintN(64, wide) === 123n);
console.log("huge-count", wide >> (1n << 2048n), -wide << -(1n << 2048n), 0n << (1n << 2048n));
const values = new Map<bigint, string>();
values.set(wide, "wide");
values.set(parsed, "equal");
values.set(123n, "low");
console.log("keys", values.size, values.get(wide), values.get(123n));
const narrowed = new BigInt64Array([wide, -wide]);
console.log("narrow", String(narrowed[0]), String(narrowed[1]));
const view = new DataView(new ArrayBuffer(8));
view.setBigInt64(0, -wide, true);
console.log("dataview", String(view.getBigInt64(0, true)));
console.log("constant-powers", 0n ** (1n << 2048n), 1n ** (1n << 2048n), (-1n) ** ((1n << 2048n) + 1n));
