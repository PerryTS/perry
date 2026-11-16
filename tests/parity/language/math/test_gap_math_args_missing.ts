// Fewer arguments than the arity: the missing ones are undefined.
const k = Number(process.argv[2] ?? "0");
const u: any = undefined;
console.log(Math.cos(), Math.sin(), Math.sqrt(), Math.floor(), Math.abs(), Math.round());
console.log(Math.atan2(1), Math.atan2(), Math.pow(2), Math.pow(), Math.imul(3), Math.imul());
console.log(Math.max(), Math.min(), Math.hypot(), Math.max(k), Math.min(k), Math.hypot(k + 3));
console.log(Math.atan2(k + 1), Math.pow(k + 2), Math.cos(u), Math.floor(u), Math.max(u), Math.hypot(u));
