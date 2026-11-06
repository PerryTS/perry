// #10418: a binary bitwise operator (`&` `|` `^` `<<` `>>`) over BigInt operands
// the compiler cannot prove statically (untyped params, `BigInt(...)`
// initializers, property/element reads, arithmetic results) was assumed to
// produce an int32 Number. `const x = a & b` took an i32 slot (read back as
// `0`), `Number(a & b)` was elided (the BigInt passed through), a
// `bigint`-typed result reached a call as `fptosi` of its box, and
// `(_2n << k) * _2n` threw "Cannot mix BigInt and other types". This is
// @noble/hashes' `fromBig` (`Number((n >> _32n) & U32_MASK64) | 0` with
// `U32_MASK64 = BigInt(2 ** 32 - 1)`) and @noble/curves' `_2n << (c1 - _1n - _1n)`.
//
// Matrix: operator x operand shape x consumer, one function per operator and
// shape, one local per consumer. Row order: typeof x, String(x), x * 2n,
// x === expected, f(x), Number(x), `let` String(x), then the same consumers
// applied to the expression itself. `+ - * ** %` and `~` are controls; the
// Number section at the end pins the int32 semantics the fast path keeps.

const _0n = BigInt(0);
const _1n = BigInt(1);
const _2n = BigInt(2);
const A_LIT = 1003n;
const B_LIT = 5n;
const A_BIG = BigInt(1003);
const B_BIG = BigInt(5);
const OBJ_LIT = { a: 1003n, b: 5n };
const OBJ_BIG = { a: BigInt(1003), b: BigInt(5) };
const ARR_LIT = [1003n, 5n];
const ARR_BIG = [BigInt(1003), BigInt(5)];

function show(v) {
  return typeof v + ":" + String(v);
}

function row(...values: unknown[]): string {
  return values.map((v) => (typeof v === "string" ? v : show(v))).join(" ");
}

function and_lit(): string {
  const x1 = 1003n & 5n, x2 = 1003n & 5n, x3 = 1003n & 5n,
    x4 = 1003n & 5n, x5 = 1003n & 5n, x6 = 1003n & 5n;
  let x7 = 1003n & 5n;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1n, show(x5), Number(x6), String(x7),
    typeof (1003n & 5n), String(1003n & 5n), (1003n & 5n) * _2n, (1003n & 5n) === 1n,
    show(1003n & 5n), Number(1003n & 5n));
}
function and_constLit(): string {
  const a = 1003n, b = 5n;
  const x1 = a & b, x2 = a & b, x3 = a & b, x4 = a & b, x5 = a & b, x6 = a & b;
  let x7 = a & b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1n, show(x5), Number(x6), String(x7),
    typeof (a & b), String(a & b), (a & b) * _2n, (a & b) === 1n, show(a & b), Number(a & b));
}
function and_letLit(): string {
  let a = 1003n, b = 5n;
  const x1 = a & b, x2 = a & b, x3 = a & b, x4 = a & b, x5 = a & b, x6 = a & b;
  let x7 = a & b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1n, show(x5), Number(x6), String(x7),
    typeof (a & b), String(a & b), (a & b) * _2n, (a & b) === 1n, show(a & b), Number(a & b));
}
function and_constBig(): string {
  const a = BigInt(1003), b = BigInt(5);
  const x1 = a & b, x2 = a & b, x3 = a & b, x4 = a & b, x5 = a & b, x6 = a & b;
  let x7 = a & b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1n, show(x5), Number(x6), String(x7),
    typeof (a & b), String(a & b), (a & b) * _2n, (a & b) === 1n, show(a & b), Number(a & b));
}
function and_letBig(): string {
  let a = BigInt(1003), b = BigInt(5);
  const x1 = a & b, x2 = a & b, x3 = a & b, x4 = a & b, x5 = a & b, x6 = a & b;
  let x7 = a & b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1n, show(x5), Number(x6), String(x7),
    typeof (a & b), String(a & b), (a & b) * _2n, (a & b) === 1n, show(a & b), Number(a & b));
}
function and_typedParam(a: bigint, b: bigint): string {
  const x1 = a & b, x2 = a & b, x3 = a & b, x4 = a & b, x5 = a & b, x6 = a & b;
  let x7 = a & b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1n, show(x5), Number(x6), String(x7),
    typeof (a & b), String(a & b), (a & b) * _2n, (a & b) === 1n, show(a & b), Number(a & b));
}
function and_untypedParam(a, b): string {
  const x1 = a & b, x2 = a & b, x3 = a & b, x4 = a & b, x5 = a & b, x6 = a & b;
  let x7 = a & b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1n, show(x5), Number(x6), String(x7),
    typeof (a & b), String(a & b), (a & b) * _2n, (a & b) === 1n, show(a & b), Number(a & b));
}
function and_moduleLit(): string {
  const x1 = A_LIT & B_LIT, x2 = A_LIT & B_LIT, x3 = A_LIT & B_LIT,
    x4 = A_LIT & B_LIT, x5 = A_LIT & B_LIT, x6 = A_LIT & B_LIT;
  let x7 = A_LIT & B_LIT;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1n, show(x5), Number(x6), String(x7),
    typeof (A_LIT & B_LIT), String(A_LIT & B_LIT), (A_LIT & B_LIT) * _2n, (A_LIT & B_LIT) === 1n,
    show(A_LIT & B_LIT), Number(A_LIT & B_LIT));
}
function and_moduleBig(): string {
  const x1 = A_BIG & B_BIG, x2 = A_BIG & B_BIG, x3 = A_BIG & B_BIG,
    x4 = A_BIG & B_BIG, x5 = A_BIG & B_BIG, x6 = A_BIG & B_BIG;
  let x7 = A_BIG & B_BIG;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1n, show(x5), Number(x6), String(x7),
    typeof (A_BIG & B_BIG), String(A_BIG & B_BIG), (A_BIG & B_BIG) * _2n, (A_BIG & B_BIG) === 1n,
    show(A_BIG & B_BIG), Number(A_BIG & B_BIG));
}
function and_propertyLit(o): string {
  const x1 = o.a & o.b, x2 = o.a & o.b, x3 = o.a & o.b,
    x4 = o.a & o.b, x5 = o.a & o.b, x6 = o.a & o.b;
  let x7 = o.a & o.b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1n, show(x5), Number(x6), String(x7),
    typeof (o.a & o.b), String(o.a & o.b), (o.a & o.b) * _2n, (o.a & o.b) === 1n, show(o.a & o.b),
    Number(o.a & o.b));
}
function and_propertyBig(o): string {
  const x1 = o.a & o.b, x2 = o.a & o.b, x3 = o.a & o.b,
    x4 = o.a & o.b, x5 = o.a & o.b, x6 = o.a & o.b;
  let x7 = o.a & o.b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1n, show(x5), Number(x6), String(x7),
    typeof (o.a & o.b), String(o.a & o.b), (o.a & o.b) * _2n, (o.a & o.b) === 1n, show(o.a & o.b),
    Number(o.a & o.b));
}
function and_elementLit(v): string {
  const x1 = v[0] & v[1], x2 = v[0] & v[1], x3 = v[0] & v[1],
    x4 = v[0] & v[1], x5 = v[0] & v[1], x6 = v[0] & v[1];
  let x7 = v[0] & v[1];
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1n, show(x5), Number(x6), String(x7),
    typeof (v[0] & v[1]), String(v[0] & v[1]), (v[0] & v[1]) * _2n, (v[0] & v[1]) === 1n,
    show(v[0] & v[1]), Number(v[0] & v[1]));
}
function and_elementBig(v): string {
  const x1 = v[0] & v[1], x2 = v[0] & v[1], x3 = v[0] & v[1],
    x4 = v[0] & v[1], x5 = v[0] & v[1], x6 = v[0] & v[1];
  let x7 = v[0] & v[1];
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1n, show(x5), Number(x6), String(x7),
    typeof (v[0] & v[1]), String(v[0] & v[1]), (v[0] & v[1]) * _2n, (v[0] & v[1]) === 1n,
    show(v[0] & v[1]), Number(v[0] & v[1]));
}
function and_arith(p, q): string {
  const x1 = (p + _0n) & (q + _0n), x2 = (p + _0n) & (q + _0n), x3 = (p + _0n) & (q + _0n),
    x4 = (p + _0n) & (q + _0n), x5 = (p + _0n) & (q + _0n), x6 = (p + _0n) & (q + _0n);
  let x7 = (p + _0n) & (q + _0n);
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1n, show(x5), Number(x6), String(x7),
    typeof ((p + _0n) & (q + _0n)), String((p + _0n) & (q + _0n)), ((p + _0n) & (q + _0n)) * _2n,
    ((p + _0n) & (q + _0n)) === 1n, show((p + _0n) & (q + _0n)), Number((p + _0n) & (q + _0n)));
}
function and_inlineBig(): string {
  const x1 = BigInt(1003) & BigInt(5), x2 = BigInt(1003) & BigInt(5), x3 = BigInt(1003) & BigInt(5),
    x4 = BigInt(1003) & BigInt(5), x5 = BigInt(1003) & BigInt(5), x6 = BigInt(1003) & BigInt(5);
  let x7 = BigInt(1003) & BigInt(5);
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1n, show(x5), Number(x6), String(x7),
    typeof (BigInt(1003) & BigInt(5)), String(BigInt(1003) & BigInt(5)),
    (BigInt(1003) & BigInt(5)) * _2n, (BigInt(1003) & BigInt(5)) === 1n,
    show(BigInt(1003) & BigInt(5)), Number(BigInt(1003) & BigInt(5)));
}
function and_paramAndLit(a): string {
  const x1 = a & 5n, x2 = a & 5n, x3 = a & 5n, x4 = a & 5n, x5 = a & 5n, x6 = a & 5n;
  let x7 = a & 5n;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1n, show(x5), Number(x6), String(x7),
    typeof (a & 5n), String(a & 5n), (a & 5n) * _2n, (a & 5n) === 1n, show(a & 5n),
    Number(a & 5n));
}
function or_lit(): string {
  const x1 = 1003n | 5n, x2 = 1003n | 5n, x3 = 1003n | 5n,
    x4 = 1003n | 5n, x5 = 1003n | 5n, x6 = 1003n | 5n;
  let x7 = 1003n | 5n;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1007n, show(x5), Number(x6), String(x7),
    typeof (1003n | 5n), String(1003n | 5n), (1003n | 5n) * _2n, (1003n | 5n) === 1007n,
    show(1003n | 5n), Number(1003n | 5n));
}
function or_constLit(): string {
  const a = 1003n, b = 5n;
  const x1 = a | b, x2 = a | b, x3 = a | b, x4 = a | b, x5 = a | b, x6 = a | b;
  let x7 = a | b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1007n, show(x5), Number(x6), String(x7),
    typeof (a | b), String(a | b), (a | b) * _2n, (a | b) === 1007n, show(a | b), Number(a | b));
}
function or_letLit(): string {
  let a = 1003n, b = 5n;
  const x1 = a | b, x2 = a | b, x3 = a | b, x4 = a | b, x5 = a | b, x6 = a | b;
  let x7 = a | b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1007n, show(x5), Number(x6), String(x7),
    typeof (a | b), String(a | b), (a | b) * _2n, (a | b) === 1007n, show(a | b), Number(a | b));
}
function or_constBig(): string {
  const a = BigInt(1003), b = BigInt(5);
  const x1 = a | b, x2 = a | b, x3 = a | b, x4 = a | b, x5 = a | b, x6 = a | b;
  let x7 = a | b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1007n, show(x5), Number(x6), String(x7),
    typeof (a | b), String(a | b), (a | b) * _2n, (a | b) === 1007n, show(a | b), Number(a | b));
}
function or_letBig(): string {
  let a = BigInt(1003), b = BigInt(5);
  const x1 = a | b, x2 = a | b, x3 = a | b, x4 = a | b, x5 = a | b, x6 = a | b;
  let x7 = a | b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1007n, show(x5), Number(x6), String(x7),
    typeof (a | b), String(a | b), (a | b) * _2n, (a | b) === 1007n, show(a | b), Number(a | b));
}
function or_typedParam(a: bigint, b: bigint): string {
  const x1 = a | b, x2 = a | b, x3 = a | b, x4 = a | b, x5 = a | b, x6 = a | b;
  let x7 = a | b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1007n, show(x5), Number(x6), String(x7),
    typeof (a | b), String(a | b), (a | b) * _2n, (a | b) === 1007n, show(a | b), Number(a | b));
}
function or_untypedParam(a, b): string {
  const x1 = a | b, x2 = a | b, x3 = a | b, x4 = a | b, x5 = a | b, x6 = a | b;
  let x7 = a | b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1007n, show(x5), Number(x6), String(x7),
    typeof (a | b), String(a | b), (a | b) * _2n, (a | b) === 1007n, show(a | b), Number(a | b));
}
function or_moduleLit(): string {
  const x1 = A_LIT | B_LIT, x2 = A_LIT | B_LIT, x3 = A_LIT | B_LIT,
    x4 = A_LIT | B_LIT, x5 = A_LIT | B_LIT, x6 = A_LIT | B_LIT;
  let x7 = A_LIT | B_LIT;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1007n, show(x5), Number(x6), String(x7),
    typeof (A_LIT | B_LIT), String(A_LIT | B_LIT), (A_LIT | B_LIT) * _2n,
    (A_LIT | B_LIT) === 1007n, show(A_LIT | B_LIT), Number(A_LIT | B_LIT));
}
function or_moduleBig(): string {
  const x1 = A_BIG | B_BIG, x2 = A_BIG | B_BIG, x3 = A_BIG | B_BIG,
    x4 = A_BIG | B_BIG, x5 = A_BIG | B_BIG, x6 = A_BIG | B_BIG;
  let x7 = A_BIG | B_BIG;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1007n, show(x5), Number(x6), String(x7),
    typeof (A_BIG | B_BIG), String(A_BIG | B_BIG), (A_BIG | B_BIG) * _2n,
    (A_BIG | B_BIG) === 1007n, show(A_BIG | B_BIG), Number(A_BIG | B_BIG));
}
function or_propertyLit(o): string {
  const x1 = o.a | o.b, x2 = o.a | o.b, x3 = o.a | o.b,
    x4 = o.a | o.b, x5 = o.a | o.b, x6 = o.a | o.b;
  let x7 = o.a | o.b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1007n, show(x5), Number(x6), String(x7),
    typeof (o.a | o.b), String(o.a | o.b), (o.a | o.b) * _2n, (o.a | o.b) === 1007n,
    show(o.a | o.b), Number(o.a | o.b));
}
function or_propertyBig(o): string {
  const x1 = o.a | o.b, x2 = o.a | o.b, x3 = o.a | o.b,
    x4 = o.a | o.b, x5 = o.a | o.b, x6 = o.a | o.b;
  let x7 = o.a | o.b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1007n, show(x5), Number(x6), String(x7),
    typeof (o.a | o.b), String(o.a | o.b), (o.a | o.b) * _2n, (o.a | o.b) === 1007n,
    show(o.a | o.b), Number(o.a | o.b));
}
function or_elementLit(v): string {
  const x1 = v[0] | v[1], x2 = v[0] | v[1], x3 = v[0] | v[1],
    x4 = v[0] | v[1], x5 = v[0] | v[1], x6 = v[0] | v[1];
  let x7 = v[0] | v[1];
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1007n, show(x5), Number(x6), String(x7),
    typeof (v[0] | v[1]), String(v[0] | v[1]), (v[0] | v[1]) * _2n, (v[0] | v[1]) === 1007n,
    show(v[0] | v[1]), Number(v[0] | v[1]));
}
function or_elementBig(v): string {
  const x1 = v[0] | v[1], x2 = v[0] | v[1], x3 = v[0] | v[1],
    x4 = v[0] | v[1], x5 = v[0] | v[1], x6 = v[0] | v[1];
  let x7 = v[0] | v[1];
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1007n, show(x5), Number(x6), String(x7),
    typeof (v[0] | v[1]), String(v[0] | v[1]), (v[0] | v[1]) * _2n, (v[0] | v[1]) === 1007n,
    show(v[0] | v[1]), Number(v[0] | v[1]));
}
function or_arith(p, q): string {
  const x1 = (p + _0n) | (q + _0n), x2 = (p + _0n) | (q + _0n), x3 = (p + _0n) | (q + _0n),
    x4 = (p + _0n) | (q + _0n), x5 = (p + _0n) | (q + _0n), x6 = (p + _0n) | (q + _0n);
  let x7 = (p + _0n) | (q + _0n);
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1007n, show(x5), Number(x6), String(x7),
    typeof ((p + _0n) | (q + _0n)), String((p + _0n) | (q + _0n)), ((p + _0n) | (q + _0n)) * _2n,
    ((p + _0n) | (q + _0n)) === 1007n, show((p + _0n) | (q + _0n)), Number((p + _0n) | (q + _0n)));
}
function or_inlineBig(): string {
  const x1 = BigInt(1003) | BigInt(5), x2 = BigInt(1003) | BigInt(5), x3 = BigInt(1003) | BigInt(5),
    x4 = BigInt(1003) | BigInt(5), x5 = BigInt(1003) | BigInt(5), x6 = BigInt(1003) | BigInt(5);
  let x7 = BigInt(1003) | BigInt(5);
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1007n, show(x5), Number(x6), String(x7),
    typeof (BigInt(1003) | BigInt(5)), String(BigInt(1003) | BigInt(5)),
    (BigInt(1003) | BigInt(5)) * _2n, (BigInt(1003) | BigInt(5)) === 1007n,
    show(BigInt(1003) | BigInt(5)), Number(BigInt(1003) | BigInt(5)));
}
function or_paramAndLit(a): string {
  const x1 = a | 5n, x2 = a | 5n, x3 = a | 5n, x4 = a | 5n, x5 = a | 5n, x6 = a | 5n;
  let x7 = a | 5n;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1007n, show(x5), Number(x6), String(x7),
    typeof (a | 5n), String(a | 5n), (a | 5n) * _2n, (a | 5n) === 1007n, show(a | 5n),
    Number(a | 5n));
}
function xor_lit(): string {
  const x1 = 1003n ^ 5n, x2 = 1003n ^ 5n, x3 = 1003n ^ 5n,
    x4 = 1003n ^ 5n, x5 = 1003n ^ 5n, x6 = 1003n ^ 5n;
  let x7 = 1003n ^ 5n;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1006n, show(x5), Number(x6), String(x7),
    typeof (1003n ^ 5n), String(1003n ^ 5n), (1003n ^ 5n) * _2n, (1003n ^ 5n) === 1006n,
    show(1003n ^ 5n), Number(1003n ^ 5n));
}
function xor_constLit(): string {
  const a = 1003n, b = 5n;
  const x1 = a ^ b, x2 = a ^ b, x3 = a ^ b, x4 = a ^ b, x5 = a ^ b, x6 = a ^ b;
  let x7 = a ^ b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1006n, show(x5), Number(x6), String(x7),
    typeof (a ^ b), String(a ^ b), (a ^ b) * _2n, (a ^ b) === 1006n, show(a ^ b), Number(a ^ b));
}
function xor_letLit(): string {
  let a = 1003n, b = 5n;
  const x1 = a ^ b, x2 = a ^ b, x3 = a ^ b, x4 = a ^ b, x5 = a ^ b, x6 = a ^ b;
  let x7 = a ^ b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1006n, show(x5), Number(x6), String(x7),
    typeof (a ^ b), String(a ^ b), (a ^ b) * _2n, (a ^ b) === 1006n, show(a ^ b), Number(a ^ b));
}
function xor_constBig(): string {
  const a = BigInt(1003), b = BigInt(5);
  const x1 = a ^ b, x2 = a ^ b, x3 = a ^ b, x4 = a ^ b, x5 = a ^ b, x6 = a ^ b;
  let x7 = a ^ b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1006n, show(x5), Number(x6), String(x7),
    typeof (a ^ b), String(a ^ b), (a ^ b) * _2n, (a ^ b) === 1006n, show(a ^ b), Number(a ^ b));
}
function xor_letBig(): string {
  let a = BigInt(1003), b = BigInt(5);
  const x1 = a ^ b, x2 = a ^ b, x3 = a ^ b, x4 = a ^ b, x5 = a ^ b, x6 = a ^ b;
  let x7 = a ^ b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1006n, show(x5), Number(x6), String(x7),
    typeof (a ^ b), String(a ^ b), (a ^ b) * _2n, (a ^ b) === 1006n, show(a ^ b), Number(a ^ b));
}
function xor_typedParam(a: bigint, b: bigint): string {
  const x1 = a ^ b, x2 = a ^ b, x3 = a ^ b, x4 = a ^ b, x5 = a ^ b, x6 = a ^ b;
  let x7 = a ^ b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1006n, show(x5), Number(x6), String(x7),
    typeof (a ^ b), String(a ^ b), (a ^ b) * _2n, (a ^ b) === 1006n, show(a ^ b), Number(a ^ b));
}
function xor_untypedParam(a, b): string {
  const x1 = a ^ b, x2 = a ^ b, x3 = a ^ b, x4 = a ^ b, x5 = a ^ b, x6 = a ^ b;
  let x7 = a ^ b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1006n, show(x5), Number(x6), String(x7),
    typeof (a ^ b), String(a ^ b), (a ^ b) * _2n, (a ^ b) === 1006n, show(a ^ b), Number(a ^ b));
}
function xor_moduleLit(): string {
  const x1 = A_LIT ^ B_LIT, x2 = A_LIT ^ B_LIT, x3 = A_LIT ^ B_LIT,
    x4 = A_LIT ^ B_LIT, x5 = A_LIT ^ B_LIT, x6 = A_LIT ^ B_LIT;
  let x7 = A_LIT ^ B_LIT;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1006n, show(x5), Number(x6), String(x7),
    typeof (A_LIT ^ B_LIT), String(A_LIT ^ B_LIT), (A_LIT ^ B_LIT) * _2n,
    (A_LIT ^ B_LIT) === 1006n, show(A_LIT ^ B_LIT), Number(A_LIT ^ B_LIT));
}
function xor_moduleBig(): string {
  const x1 = A_BIG ^ B_BIG, x2 = A_BIG ^ B_BIG, x3 = A_BIG ^ B_BIG,
    x4 = A_BIG ^ B_BIG, x5 = A_BIG ^ B_BIG, x6 = A_BIG ^ B_BIG;
  let x7 = A_BIG ^ B_BIG;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1006n, show(x5), Number(x6), String(x7),
    typeof (A_BIG ^ B_BIG), String(A_BIG ^ B_BIG), (A_BIG ^ B_BIG) * _2n,
    (A_BIG ^ B_BIG) === 1006n, show(A_BIG ^ B_BIG), Number(A_BIG ^ B_BIG));
}
function xor_propertyLit(o): string {
  const x1 = o.a ^ o.b, x2 = o.a ^ o.b, x3 = o.a ^ o.b,
    x4 = o.a ^ o.b, x5 = o.a ^ o.b, x6 = o.a ^ o.b;
  let x7 = o.a ^ o.b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1006n, show(x5), Number(x6), String(x7),
    typeof (o.a ^ o.b), String(o.a ^ o.b), (o.a ^ o.b) * _2n, (o.a ^ o.b) === 1006n,
    show(o.a ^ o.b), Number(o.a ^ o.b));
}
function xor_propertyBig(o): string {
  const x1 = o.a ^ o.b, x2 = o.a ^ o.b, x3 = o.a ^ o.b,
    x4 = o.a ^ o.b, x5 = o.a ^ o.b, x6 = o.a ^ o.b;
  let x7 = o.a ^ o.b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1006n, show(x5), Number(x6), String(x7),
    typeof (o.a ^ o.b), String(o.a ^ o.b), (o.a ^ o.b) * _2n, (o.a ^ o.b) === 1006n,
    show(o.a ^ o.b), Number(o.a ^ o.b));
}
function xor_elementLit(v): string {
  const x1 = v[0] ^ v[1], x2 = v[0] ^ v[1], x3 = v[0] ^ v[1],
    x4 = v[0] ^ v[1], x5 = v[0] ^ v[1], x6 = v[0] ^ v[1];
  let x7 = v[0] ^ v[1];
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1006n, show(x5), Number(x6), String(x7),
    typeof (v[0] ^ v[1]), String(v[0] ^ v[1]), (v[0] ^ v[1]) * _2n, (v[0] ^ v[1]) === 1006n,
    show(v[0] ^ v[1]), Number(v[0] ^ v[1]));
}
function xor_elementBig(v): string {
  const x1 = v[0] ^ v[1], x2 = v[0] ^ v[1], x3 = v[0] ^ v[1],
    x4 = v[0] ^ v[1], x5 = v[0] ^ v[1], x6 = v[0] ^ v[1];
  let x7 = v[0] ^ v[1];
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1006n, show(x5), Number(x6), String(x7),
    typeof (v[0] ^ v[1]), String(v[0] ^ v[1]), (v[0] ^ v[1]) * _2n, (v[0] ^ v[1]) === 1006n,
    show(v[0] ^ v[1]), Number(v[0] ^ v[1]));
}
function xor_arith(p, q): string {
  const x1 = (p + _0n) ^ (q + _0n), x2 = (p + _0n) ^ (q + _0n), x3 = (p + _0n) ^ (q + _0n),
    x4 = (p + _0n) ^ (q + _0n), x5 = (p + _0n) ^ (q + _0n), x6 = (p + _0n) ^ (q + _0n);
  let x7 = (p + _0n) ^ (q + _0n);
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1006n, show(x5), Number(x6), String(x7),
    typeof ((p + _0n) ^ (q + _0n)), String((p + _0n) ^ (q + _0n)), ((p + _0n) ^ (q + _0n)) * _2n,
    ((p + _0n) ^ (q + _0n)) === 1006n, show((p + _0n) ^ (q + _0n)), Number((p + _0n) ^ (q + _0n)));
}
function xor_inlineBig(): string {
  const x1 = BigInt(1003) ^ BigInt(5), x2 = BigInt(1003) ^ BigInt(5), x3 = BigInt(1003) ^ BigInt(5),
    x4 = BigInt(1003) ^ BigInt(5), x5 = BigInt(1003) ^ BigInt(5), x6 = BigInt(1003) ^ BigInt(5);
  let x7 = BigInt(1003) ^ BigInt(5);
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1006n, show(x5), Number(x6), String(x7),
    typeof (BigInt(1003) ^ BigInt(5)), String(BigInt(1003) ^ BigInt(5)),
    (BigInt(1003) ^ BigInt(5)) * _2n, (BigInt(1003) ^ BigInt(5)) === 1006n,
    show(BigInt(1003) ^ BigInt(5)), Number(BigInt(1003) ^ BigInt(5)));
}
function xor_paramAndLit(a): string {
  const x1 = a ^ 5n, x2 = a ^ 5n, x3 = a ^ 5n, x4 = a ^ 5n, x5 = a ^ 5n, x6 = a ^ 5n;
  let x7 = a ^ 5n;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1006n, show(x5), Number(x6), String(x7),
    typeof (a ^ 5n), String(a ^ 5n), (a ^ 5n) * _2n, (a ^ 5n) === 1006n, show(a ^ 5n),
    Number(a ^ 5n));
}
function shl_lit(): string {
  const x1 = 1003n << 5n, x2 = 1003n << 5n, x3 = 1003n << 5n,
    x4 = 1003n << 5n, x5 = 1003n << 5n, x6 = 1003n << 5n;
  let x7 = 1003n << 5n;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 32096n, show(x5), Number(x6), String(x7),
    typeof (1003n << 5n), String(1003n << 5n), (1003n << 5n) * _2n, (1003n << 5n) === 32096n,
    show(1003n << 5n), Number(1003n << 5n));
}
function shl_constLit(): string {
  const a = 1003n, b = 5n;
  const x1 = a << b, x2 = a << b, x3 = a << b, x4 = a << b, x5 = a << b, x6 = a << b;
  let x7 = a << b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 32096n, show(x5), Number(x6), String(x7),
    typeof (a << b), String(a << b), (a << b) * _2n, (a << b) === 32096n, show(a << b),
    Number(a << b));
}
function shl_letLit(): string {
  let a = 1003n, b = 5n;
  const x1 = a << b, x2 = a << b, x3 = a << b, x4 = a << b, x5 = a << b, x6 = a << b;
  let x7 = a << b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 32096n, show(x5), Number(x6), String(x7),
    typeof (a << b), String(a << b), (a << b) * _2n, (a << b) === 32096n, show(a << b),
    Number(a << b));
}
function shl_constBig(): string {
  const a = BigInt(1003), b = BigInt(5);
  const x1 = a << b, x2 = a << b, x3 = a << b, x4 = a << b, x5 = a << b, x6 = a << b;
  let x7 = a << b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 32096n, show(x5), Number(x6), String(x7),
    typeof (a << b), String(a << b), (a << b) * _2n, (a << b) === 32096n, show(a << b),
    Number(a << b));
}
function shl_letBig(): string {
  let a = BigInt(1003), b = BigInt(5);
  const x1 = a << b, x2 = a << b, x3 = a << b, x4 = a << b, x5 = a << b, x6 = a << b;
  let x7 = a << b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 32096n, show(x5), Number(x6), String(x7),
    typeof (a << b), String(a << b), (a << b) * _2n, (a << b) === 32096n, show(a << b),
    Number(a << b));
}
function shl_typedParam(a: bigint, b: bigint): string {
  const x1 = a << b, x2 = a << b, x3 = a << b, x4 = a << b, x5 = a << b, x6 = a << b;
  let x7 = a << b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 32096n, show(x5), Number(x6), String(x7),
    typeof (a << b), String(a << b), (a << b) * _2n, (a << b) === 32096n, show(a << b),
    Number(a << b));
}
function shl_untypedParam(a, b): string {
  const x1 = a << b, x2 = a << b, x3 = a << b, x4 = a << b, x5 = a << b, x6 = a << b;
  let x7 = a << b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 32096n, show(x5), Number(x6), String(x7),
    typeof (a << b), String(a << b), (a << b) * _2n, (a << b) === 32096n, show(a << b),
    Number(a << b));
}
function shl_moduleLit(): string {
  const x1 = A_LIT << B_LIT, x2 = A_LIT << B_LIT, x3 = A_LIT << B_LIT,
    x4 = A_LIT << B_LIT, x5 = A_LIT << B_LIT, x6 = A_LIT << B_LIT;
  let x7 = A_LIT << B_LIT;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 32096n, show(x5), Number(x6), String(x7),
    typeof (A_LIT << B_LIT), String(A_LIT << B_LIT), (A_LIT << B_LIT) * _2n,
    (A_LIT << B_LIT) === 32096n, show(A_LIT << B_LIT), Number(A_LIT << B_LIT));
}
function shl_moduleBig(): string {
  const x1 = A_BIG << B_BIG, x2 = A_BIG << B_BIG, x3 = A_BIG << B_BIG,
    x4 = A_BIG << B_BIG, x5 = A_BIG << B_BIG, x6 = A_BIG << B_BIG;
  let x7 = A_BIG << B_BIG;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 32096n, show(x5), Number(x6), String(x7),
    typeof (A_BIG << B_BIG), String(A_BIG << B_BIG), (A_BIG << B_BIG) * _2n,
    (A_BIG << B_BIG) === 32096n, show(A_BIG << B_BIG), Number(A_BIG << B_BIG));
}
function shl_propertyLit(o): string {
  const x1 = o.a << o.b, x2 = o.a << o.b, x3 = o.a << o.b,
    x4 = o.a << o.b, x5 = o.a << o.b, x6 = o.a << o.b;
  let x7 = o.a << o.b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 32096n, show(x5), Number(x6), String(x7),
    typeof (o.a << o.b), String(o.a << o.b), (o.a << o.b) * _2n, (o.a << o.b) === 32096n,
    show(o.a << o.b), Number(o.a << o.b));
}
function shl_propertyBig(o): string {
  const x1 = o.a << o.b, x2 = o.a << o.b, x3 = o.a << o.b,
    x4 = o.a << o.b, x5 = o.a << o.b, x6 = o.a << o.b;
  let x7 = o.a << o.b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 32096n, show(x5), Number(x6), String(x7),
    typeof (o.a << o.b), String(o.a << o.b), (o.a << o.b) * _2n, (o.a << o.b) === 32096n,
    show(o.a << o.b), Number(o.a << o.b));
}
function shl_elementLit(v): string {
  const x1 = v[0] << v[1], x2 = v[0] << v[1], x3 = v[0] << v[1],
    x4 = v[0] << v[1], x5 = v[0] << v[1], x6 = v[0] << v[1];
  let x7 = v[0] << v[1];
  return row(typeof x1, String(x2), x3 * _2n, x4 === 32096n, show(x5), Number(x6), String(x7),
    typeof (v[0] << v[1]), String(v[0] << v[1]), (v[0] << v[1]) * _2n, (v[0] << v[1]) === 32096n,
    show(v[0] << v[1]), Number(v[0] << v[1]));
}
function shl_elementBig(v): string {
  const x1 = v[0] << v[1], x2 = v[0] << v[1], x3 = v[0] << v[1],
    x4 = v[0] << v[1], x5 = v[0] << v[1], x6 = v[0] << v[1];
  let x7 = v[0] << v[1];
  return row(typeof x1, String(x2), x3 * _2n, x4 === 32096n, show(x5), Number(x6), String(x7),
    typeof (v[0] << v[1]), String(v[0] << v[1]), (v[0] << v[1]) * _2n, (v[0] << v[1]) === 32096n,
    show(v[0] << v[1]), Number(v[0] << v[1]));
}
function shl_arith(p, q): string {
  const x1 = (p + _0n) << (q + _0n), x2 = (p + _0n) << (q + _0n), x3 = (p + _0n) << (q + _0n),
    x4 = (p + _0n) << (q + _0n), x5 = (p + _0n) << (q + _0n), x6 = (p + _0n) << (q + _0n);
  let x7 = (p + _0n) << (q + _0n);
  return row(typeof x1, String(x2), x3 * _2n, x4 === 32096n, show(x5), Number(x6), String(x7),
    typeof ((p + _0n) << (q + _0n)), String((p + _0n) << (q + _0n)),
    ((p + _0n) << (q + _0n)) * _2n, ((p + _0n) << (q + _0n)) === 32096n,
    show((p + _0n) << (q + _0n)), Number((p + _0n) << (q + _0n)));
}
function shl_inlineBig(): string {
  const x1 = BigInt(1003) << BigInt(5), x2 = BigInt(1003) << BigInt(5), x3 = BigInt(1003) << BigInt(5),
    x4 = BigInt(1003) << BigInt(5), x5 = BigInt(1003) << BigInt(5), x6 = BigInt(1003) << BigInt(5);
  let x7 = BigInt(1003) << BigInt(5);
  return row(typeof x1, String(x2), x3 * _2n, x4 === 32096n, show(x5), Number(x6), String(x7),
    typeof (BigInt(1003) << BigInt(5)), String(BigInt(1003) << BigInt(5)),
    (BigInt(1003) << BigInt(5)) * _2n, (BigInt(1003) << BigInt(5)) === 32096n,
    show(BigInt(1003) << BigInt(5)), Number(BigInt(1003) << BigInt(5)));
}
function shl_paramAndLit(a): string {
  const x1 = a << 5n, x2 = a << 5n, x3 = a << 5n, x4 = a << 5n, x5 = a << 5n, x6 = a << 5n;
  let x7 = a << 5n;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 32096n, show(x5), Number(x6), String(x7),
    typeof (a << 5n), String(a << 5n), (a << 5n) * _2n, (a << 5n) === 32096n, show(a << 5n),
    Number(a << 5n));
}
function shr_lit(): string {
  const x1 = 1003n >> 5n, x2 = 1003n >> 5n, x3 = 1003n >> 5n,
    x4 = 1003n >> 5n, x5 = 1003n >> 5n, x6 = 1003n >> 5n;
  let x7 = 1003n >> 5n;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 31n, show(x5), Number(x6), String(x7),
    typeof (1003n >> 5n), String(1003n >> 5n), (1003n >> 5n) * _2n, (1003n >> 5n) === 31n,
    show(1003n >> 5n), Number(1003n >> 5n));
}
function shr_constLit(): string {
  const a = 1003n, b = 5n;
  const x1 = a >> b, x2 = a >> b, x3 = a >> b, x4 = a >> b, x5 = a >> b, x6 = a >> b;
  let x7 = a >> b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 31n, show(x5), Number(x6), String(x7),
    typeof (a >> b), String(a >> b), (a >> b) * _2n, (a >> b) === 31n, show(a >> b),
    Number(a >> b));
}
function shr_letLit(): string {
  let a = 1003n, b = 5n;
  const x1 = a >> b, x2 = a >> b, x3 = a >> b, x4 = a >> b, x5 = a >> b, x6 = a >> b;
  let x7 = a >> b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 31n, show(x5), Number(x6), String(x7),
    typeof (a >> b), String(a >> b), (a >> b) * _2n, (a >> b) === 31n, show(a >> b),
    Number(a >> b));
}
function shr_constBig(): string {
  const a = BigInt(1003), b = BigInt(5);
  const x1 = a >> b, x2 = a >> b, x3 = a >> b, x4 = a >> b, x5 = a >> b, x6 = a >> b;
  let x7 = a >> b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 31n, show(x5), Number(x6), String(x7),
    typeof (a >> b), String(a >> b), (a >> b) * _2n, (a >> b) === 31n, show(a >> b),
    Number(a >> b));
}
function shr_letBig(): string {
  let a = BigInt(1003), b = BigInt(5);
  const x1 = a >> b, x2 = a >> b, x3 = a >> b, x4 = a >> b, x5 = a >> b, x6 = a >> b;
  let x7 = a >> b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 31n, show(x5), Number(x6), String(x7),
    typeof (a >> b), String(a >> b), (a >> b) * _2n, (a >> b) === 31n, show(a >> b),
    Number(a >> b));
}
function shr_typedParam(a: bigint, b: bigint): string {
  const x1 = a >> b, x2 = a >> b, x3 = a >> b, x4 = a >> b, x5 = a >> b, x6 = a >> b;
  let x7 = a >> b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 31n, show(x5), Number(x6), String(x7),
    typeof (a >> b), String(a >> b), (a >> b) * _2n, (a >> b) === 31n, show(a >> b),
    Number(a >> b));
}
function shr_untypedParam(a, b): string {
  const x1 = a >> b, x2 = a >> b, x3 = a >> b, x4 = a >> b, x5 = a >> b, x6 = a >> b;
  let x7 = a >> b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 31n, show(x5), Number(x6), String(x7),
    typeof (a >> b), String(a >> b), (a >> b) * _2n, (a >> b) === 31n, show(a >> b),
    Number(a >> b));
}
function shr_moduleLit(): string {
  const x1 = A_LIT >> B_LIT, x2 = A_LIT >> B_LIT, x3 = A_LIT >> B_LIT,
    x4 = A_LIT >> B_LIT, x5 = A_LIT >> B_LIT, x6 = A_LIT >> B_LIT;
  let x7 = A_LIT >> B_LIT;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 31n, show(x5), Number(x6), String(x7),
    typeof (A_LIT >> B_LIT), String(A_LIT >> B_LIT), (A_LIT >> B_LIT) * _2n,
    (A_LIT >> B_LIT) === 31n, show(A_LIT >> B_LIT), Number(A_LIT >> B_LIT));
}
function shr_moduleBig(): string {
  const x1 = A_BIG >> B_BIG, x2 = A_BIG >> B_BIG, x3 = A_BIG >> B_BIG,
    x4 = A_BIG >> B_BIG, x5 = A_BIG >> B_BIG, x6 = A_BIG >> B_BIG;
  let x7 = A_BIG >> B_BIG;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 31n, show(x5), Number(x6), String(x7),
    typeof (A_BIG >> B_BIG), String(A_BIG >> B_BIG), (A_BIG >> B_BIG) * _2n,
    (A_BIG >> B_BIG) === 31n, show(A_BIG >> B_BIG), Number(A_BIG >> B_BIG));
}
function shr_propertyLit(o): string {
  const x1 = o.a >> o.b, x2 = o.a >> o.b, x3 = o.a >> o.b,
    x4 = o.a >> o.b, x5 = o.a >> o.b, x6 = o.a >> o.b;
  let x7 = o.a >> o.b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 31n, show(x5), Number(x6), String(x7),
    typeof (o.a >> o.b), String(o.a >> o.b), (o.a >> o.b) * _2n, (o.a >> o.b) === 31n,
    show(o.a >> o.b), Number(o.a >> o.b));
}
function shr_propertyBig(o): string {
  const x1 = o.a >> o.b, x2 = o.a >> o.b, x3 = o.a >> o.b,
    x4 = o.a >> o.b, x5 = o.a >> o.b, x6 = o.a >> o.b;
  let x7 = o.a >> o.b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 31n, show(x5), Number(x6), String(x7),
    typeof (o.a >> o.b), String(o.a >> o.b), (o.a >> o.b) * _2n, (o.a >> o.b) === 31n,
    show(o.a >> o.b), Number(o.a >> o.b));
}
function shr_elementLit(v): string {
  const x1 = v[0] >> v[1], x2 = v[0] >> v[1], x3 = v[0] >> v[1],
    x4 = v[0] >> v[1], x5 = v[0] >> v[1], x6 = v[0] >> v[1];
  let x7 = v[0] >> v[1];
  return row(typeof x1, String(x2), x3 * _2n, x4 === 31n, show(x5), Number(x6), String(x7),
    typeof (v[0] >> v[1]), String(v[0] >> v[1]), (v[0] >> v[1]) * _2n, (v[0] >> v[1]) === 31n,
    show(v[0] >> v[1]), Number(v[0] >> v[1]));
}
function shr_elementBig(v): string {
  const x1 = v[0] >> v[1], x2 = v[0] >> v[1], x3 = v[0] >> v[1],
    x4 = v[0] >> v[1], x5 = v[0] >> v[1], x6 = v[0] >> v[1];
  let x7 = v[0] >> v[1];
  return row(typeof x1, String(x2), x3 * _2n, x4 === 31n, show(x5), Number(x6), String(x7),
    typeof (v[0] >> v[1]), String(v[0] >> v[1]), (v[0] >> v[1]) * _2n, (v[0] >> v[1]) === 31n,
    show(v[0] >> v[1]), Number(v[0] >> v[1]));
}
function shr_arith(p, q): string {
  const x1 = (p + _0n) >> (q + _0n), x2 = (p + _0n) >> (q + _0n), x3 = (p + _0n) >> (q + _0n),
    x4 = (p + _0n) >> (q + _0n), x5 = (p + _0n) >> (q + _0n), x6 = (p + _0n) >> (q + _0n);
  let x7 = (p + _0n) >> (q + _0n);
  return row(typeof x1, String(x2), x3 * _2n, x4 === 31n, show(x5), Number(x6), String(x7),
    typeof ((p + _0n) >> (q + _0n)), String((p + _0n) >> (q + _0n)),
    ((p + _0n) >> (q + _0n)) * _2n, ((p + _0n) >> (q + _0n)) === 31n, show((p + _0n) >> (q + _0n)),
    Number((p + _0n) >> (q + _0n)));
}
function shr_inlineBig(): string {
  const x1 = BigInt(1003) >> BigInt(5), x2 = BigInt(1003) >> BigInt(5), x3 = BigInt(1003) >> BigInt(5),
    x4 = BigInt(1003) >> BigInt(5), x5 = BigInt(1003) >> BigInt(5), x6 = BigInt(1003) >> BigInt(5);
  let x7 = BigInt(1003) >> BigInt(5);
  return row(typeof x1, String(x2), x3 * _2n, x4 === 31n, show(x5), Number(x6), String(x7),
    typeof (BigInt(1003) >> BigInt(5)), String(BigInt(1003) >> BigInt(5)),
    (BigInt(1003) >> BigInt(5)) * _2n, (BigInt(1003) >> BigInt(5)) === 31n,
    show(BigInt(1003) >> BigInt(5)), Number(BigInt(1003) >> BigInt(5)));
}
function shr_paramAndLit(a): string {
  const x1 = a >> 5n, x2 = a >> 5n, x3 = a >> 5n, x4 = a >> 5n, x5 = a >> 5n, x6 = a >> 5n;
  let x7 = a >> 5n;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 31n, show(x5), Number(x6), String(x7),
    typeof (a >> 5n), String(a >> 5n), (a >> 5n) * _2n, (a >> 5n) === 31n, show(a >> 5n),
    Number(a >> 5n));
}
function add_typedParam(a: bigint, b: bigint): string {
  const x1 = a + b, x2 = a + b, x3 = a + b, x4 = a + b, x5 = a + b, x6 = a + b;
  let x7 = a + b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1008n, show(x5), Number(x6), String(x7),
    typeof (a + b), String(a + b), (a + b) * _2n, (a + b) === 1008n, show(a + b), Number(a + b));
}
function add_untypedParam(a, b): string {
  const x1 = a + b, x2 = a + b, x3 = a + b, x4 = a + b, x5 = a + b, x6 = a + b;
  let x7 = a + b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1008n, show(x5), Number(x6), String(x7),
    typeof (a + b), String(a + b), (a + b) * _2n, (a + b) === 1008n, show(a + b), Number(a + b));
}
function add_moduleBig(): string {
  const x1 = A_BIG + B_BIG, x2 = A_BIG + B_BIG, x3 = A_BIG + B_BIG,
    x4 = A_BIG + B_BIG, x5 = A_BIG + B_BIG, x6 = A_BIG + B_BIG;
  let x7 = A_BIG + B_BIG;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1008n, show(x5), Number(x6), String(x7),
    typeof (A_BIG + B_BIG), String(A_BIG + B_BIG), (A_BIG + B_BIG) * _2n,
    (A_BIG + B_BIG) === 1008n, show(A_BIG + B_BIG), Number(A_BIG + B_BIG));
}
function sub_typedParam(a: bigint, b: bigint): string {
  const x1 = a - b, x2 = a - b, x3 = a - b, x4 = a - b, x5 = a - b, x6 = a - b;
  let x7 = a - b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 998n, show(x5), Number(x6), String(x7),
    typeof (a - b), String(a - b), (a - b) * _2n, (a - b) === 998n, show(a - b), Number(a - b));
}
function sub_untypedParam(a, b): string {
  const x1 = a - b, x2 = a - b, x3 = a - b, x4 = a - b, x5 = a - b, x6 = a - b;
  let x7 = a - b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 998n, show(x5), Number(x6), String(x7),
    typeof (a - b), String(a - b), (a - b) * _2n, (a - b) === 998n, show(a - b), Number(a - b));
}
function sub_moduleBig(): string {
  const x1 = A_BIG - B_BIG, x2 = A_BIG - B_BIG, x3 = A_BIG - B_BIG,
    x4 = A_BIG - B_BIG, x5 = A_BIG - B_BIG, x6 = A_BIG - B_BIG;
  let x7 = A_BIG - B_BIG;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 998n, show(x5), Number(x6), String(x7),
    typeof (A_BIG - B_BIG), String(A_BIG - B_BIG), (A_BIG - B_BIG) * _2n, (A_BIG - B_BIG) === 998n,
    show(A_BIG - B_BIG), Number(A_BIG - B_BIG));
}
function mul_typedParam(a: bigint, b: bigint): string {
  const x1 = a * b, x2 = a * b, x3 = a * b, x4 = a * b, x5 = a * b, x6 = a * b;
  let x7 = a * b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 5015n, show(x5), Number(x6), String(x7),
    typeof (a * b), String(a * b), (a * b) * _2n, (a * b) === 5015n, show(a * b), Number(a * b));
}
function mul_untypedParam(a, b): string {
  const x1 = a * b, x2 = a * b, x3 = a * b, x4 = a * b, x5 = a * b, x6 = a * b;
  let x7 = a * b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 5015n, show(x5), Number(x6), String(x7),
    typeof (a * b), String(a * b), (a * b) * _2n, (a * b) === 5015n, show(a * b), Number(a * b));
}
function mul_moduleBig(): string {
  const x1 = A_BIG * B_BIG, x2 = A_BIG * B_BIG, x3 = A_BIG * B_BIG,
    x4 = A_BIG * B_BIG, x5 = A_BIG * B_BIG, x6 = A_BIG * B_BIG;
  let x7 = A_BIG * B_BIG;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 5015n, show(x5), Number(x6), String(x7),
    typeof (A_BIG * B_BIG), String(A_BIG * B_BIG), (A_BIG * B_BIG) * _2n,
    (A_BIG * B_BIG) === 5015n, show(A_BIG * B_BIG), Number(A_BIG * B_BIG));
}
function pow_typedParam(a: bigint, b: bigint): string {
  const x1 = a ** b, x2 = a ** b, x3 = a ** b, x4 = a ** b, x5 = a ** b, x6 = a ** b;
  let x7 = a ** b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1015090270405243n, show(x5), Number(x6),
    String(x7), typeof (a ** b), String(a ** b), (a ** b) * _2n, (a ** b) === 1015090270405243n,
    show(a ** b), Number(a ** b));
}
function pow_untypedParam(a, b): string {
  const x1 = a ** b, x2 = a ** b, x3 = a ** b, x4 = a ** b, x5 = a ** b, x6 = a ** b;
  let x7 = a ** b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1015090270405243n, show(x5), Number(x6),
    String(x7), typeof (a ** b), String(a ** b), (a ** b) * _2n, (a ** b) === 1015090270405243n,
    show(a ** b), Number(a ** b));
}
function pow_moduleBig(): string {
  const x1 = A_BIG ** B_BIG, x2 = A_BIG ** B_BIG, x3 = A_BIG ** B_BIG,
    x4 = A_BIG ** B_BIG, x5 = A_BIG ** B_BIG, x6 = A_BIG ** B_BIG;
  let x7 = A_BIG ** B_BIG;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 1015090270405243n, show(x5), Number(x6),
    String(x7), typeof (A_BIG ** B_BIG), String(A_BIG ** B_BIG), (A_BIG ** B_BIG) * _2n,
    (A_BIG ** B_BIG) === 1015090270405243n, show(A_BIG ** B_BIG), Number(A_BIG ** B_BIG));
}
function mod_typedParam(a: bigint, b: bigint): string {
  const x1 = a % b, x2 = a % b, x3 = a % b, x4 = a % b, x5 = a % b, x6 = a % b;
  let x7 = a % b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 3n, show(x5), Number(x6), String(x7),
    typeof (a % b), String(a % b), (a % b) * _2n, (a % b) === 3n, show(a % b), Number(a % b));
}
function mod_untypedParam(a, b): string {
  const x1 = a % b, x2 = a % b, x3 = a % b, x4 = a % b, x5 = a % b, x6 = a % b;
  let x7 = a % b;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 3n, show(x5), Number(x6), String(x7),
    typeof (a % b), String(a % b), (a % b) * _2n, (a % b) === 3n, show(a % b), Number(a % b));
}
function mod_moduleBig(): string {
  const x1 = A_BIG % B_BIG, x2 = A_BIG % B_BIG, x3 = A_BIG % B_BIG,
    x4 = A_BIG % B_BIG, x5 = A_BIG % B_BIG, x6 = A_BIG % B_BIG;
  let x7 = A_BIG % B_BIG;
  return row(typeof x1, String(x2), x3 * _2n, x4 === 3n, show(x5), Number(x6), String(x7),
    typeof (A_BIG % B_BIG), String(A_BIG % B_BIG), (A_BIG % B_BIG) * _2n, (A_BIG % B_BIG) === 3n,
    show(A_BIG % B_BIG), Number(A_BIG % B_BIG));
}
function and_closure(a, b): string {
  return show((() => a & b)());
}
function or_closure(a, b): string {
  return show((() => a | b)());
}
function xor_closure(a, b): string {
  return show((() => a ^ b)());
}
function shl_closure(a, b): string {
  return show((() => a << b)());
}
function shr_closure(a, b): string {
  return show((() => a >> b)());
}
function bitnot_untypedParam(a): string {
  const x1 = ~a, x2 = ~a, x3 = ~a;
  return row(show(x1), x2 === -1004n, Number(x3), Number(~a), (~a) * _2n);
}
function bitnot_typedParam(a: bigint): string {
  const x1 = ~a, x2 = ~a, x3 = ~a;
  return row(show(x1), x2 === -1004n, Number(x3), Number(~a), (~a) * _2n);
}
function bitnot_moduleBig(): string {
  const x1 = ~A_BIG, x2 = ~A_BIG, x3 = ~A_BIG;
  return row(show(x1), x2 === -1004n, Number(x3), Number(~A_BIG), (~A_BIG) * _2n);
}

const cases: Array<[string, () => string]> = [
  ["and_lit", () => and_lit()],
  ["and_constLit", () => and_constLit()],
  ["and_letLit", () => and_letLit()],
  ["and_constBig", () => and_constBig()],
  ["and_letBig", () => and_letBig()],
  ["and_typedParam", () => and_typedParam(BigInt(1003), BigInt(5))],
  ["and_untypedParam", () => and_untypedParam(BigInt(1003), BigInt(5))],
  ["and_moduleLit", () => and_moduleLit()],
  ["and_moduleBig", () => and_moduleBig()],
  ["and_propertyLit", () => and_propertyLit(OBJ_LIT)],
  ["and_propertyBig", () => and_propertyBig(OBJ_BIG)],
  ["and_elementLit", () => and_elementLit(ARR_LIT)],
  ["and_elementBig", () => and_elementBig(ARR_BIG)],
  ["and_arith", () => and_arith(BigInt(1003), BigInt(5))],
  ["and_inlineBig", () => and_inlineBig()],
  ["and_paramAndLit", () => and_paramAndLit(BigInt(1003))],
  ["or_lit", () => or_lit()],
  ["or_constLit", () => or_constLit()],
  ["or_letLit", () => or_letLit()],
  ["or_constBig", () => or_constBig()],
  ["or_letBig", () => or_letBig()],
  ["or_typedParam", () => or_typedParam(BigInt(1003), BigInt(5))],
  ["or_untypedParam", () => or_untypedParam(BigInt(1003), BigInt(5))],
  ["or_moduleLit", () => or_moduleLit()],
  ["or_moduleBig", () => or_moduleBig()],
  ["or_propertyLit", () => or_propertyLit(OBJ_LIT)],
  ["or_propertyBig", () => or_propertyBig(OBJ_BIG)],
  ["or_elementLit", () => or_elementLit(ARR_LIT)],
  ["or_elementBig", () => or_elementBig(ARR_BIG)],
  ["or_arith", () => or_arith(BigInt(1003), BigInt(5))],
  ["or_inlineBig", () => or_inlineBig()],
  ["or_paramAndLit", () => or_paramAndLit(BigInt(1003))],
  ["xor_lit", () => xor_lit()],
  ["xor_constLit", () => xor_constLit()],
  ["xor_letLit", () => xor_letLit()],
  ["xor_constBig", () => xor_constBig()],
  ["xor_letBig", () => xor_letBig()],
  ["xor_typedParam", () => xor_typedParam(BigInt(1003), BigInt(5))],
  ["xor_untypedParam", () => xor_untypedParam(BigInt(1003), BigInt(5))],
  ["xor_moduleLit", () => xor_moduleLit()],
  ["xor_moduleBig", () => xor_moduleBig()],
  ["xor_propertyLit", () => xor_propertyLit(OBJ_LIT)],
  ["xor_propertyBig", () => xor_propertyBig(OBJ_BIG)],
  ["xor_elementLit", () => xor_elementLit(ARR_LIT)],
  ["xor_elementBig", () => xor_elementBig(ARR_BIG)],
  ["xor_arith", () => xor_arith(BigInt(1003), BigInt(5))],
  ["xor_inlineBig", () => xor_inlineBig()],
  ["xor_paramAndLit", () => xor_paramAndLit(BigInt(1003))],
  ["shl_lit", () => shl_lit()],
  ["shl_constLit", () => shl_constLit()],
  ["shl_letLit", () => shl_letLit()],
  ["shl_constBig", () => shl_constBig()],
  ["shl_letBig", () => shl_letBig()],
  ["shl_typedParam", () => shl_typedParam(BigInt(1003), BigInt(5))],
  ["shl_untypedParam", () => shl_untypedParam(BigInt(1003), BigInt(5))],
  ["shl_moduleLit", () => shl_moduleLit()],
  ["shl_moduleBig", () => shl_moduleBig()],
  ["shl_propertyLit", () => shl_propertyLit(OBJ_LIT)],
  ["shl_propertyBig", () => shl_propertyBig(OBJ_BIG)],
  ["shl_elementLit", () => shl_elementLit(ARR_LIT)],
  ["shl_elementBig", () => shl_elementBig(ARR_BIG)],
  ["shl_arith", () => shl_arith(BigInt(1003), BigInt(5))],
  ["shl_inlineBig", () => shl_inlineBig()],
  ["shl_paramAndLit", () => shl_paramAndLit(BigInt(1003))],
  ["shr_lit", () => shr_lit()],
  ["shr_constLit", () => shr_constLit()],
  ["shr_letLit", () => shr_letLit()],
  ["shr_constBig", () => shr_constBig()],
  ["shr_letBig", () => shr_letBig()],
  ["shr_typedParam", () => shr_typedParam(BigInt(1003), BigInt(5))],
  ["shr_untypedParam", () => shr_untypedParam(BigInt(1003), BigInt(5))],
  ["shr_moduleLit", () => shr_moduleLit()],
  ["shr_moduleBig", () => shr_moduleBig()],
  ["shr_propertyLit", () => shr_propertyLit(OBJ_LIT)],
  ["shr_propertyBig", () => shr_propertyBig(OBJ_BIG)],
  ["shr_elementLit", () => shr_elementLit(ARR_LIT)],
  ["shr_elementBig", () => shr_elementBig(ARR_BIG)],
  ["shr_arith", () => shr_arith(BigInt(1003), BigInt(5))],
  ["shr_inlineBig", () => shr_inlineBig()],
  ["shr_paramAndLit", () => shr_paramAndLit(BigInt(1003))],
  ["add_typedParam", () => add_typedParam(BigInt(1003), BigInt(5))],
  ["add_untypedParam", () => add_untypedParam(BigInt(1003), BigInt(5))],
  ["add_moduleBig", () => add_moduleBig()],
  ["sub_typedParam", () => sub_typedParam(BigInt(1003), BigInt(5))],
  ["sub_untypedParam", () => sub_untypedParam(BigInt(1003), BigInt(5))],
  ["sub_moduleBig", () => sub_moduleBig()],
  ["mul_typedParam", () => mul_typedParam(BigInt(1003), BigInt(5))],
  ["mul_untypedParam", () => mul_untypedParam(BigInt(1003), BigInt(5))],
  ["mul_moduleBig", () => mul_moduleBig()],
  ["pow_typedParam", () => pow_typedParam(BigInt(1003), BigInt(5))],
  ["pow_untypedParam", () => pow_untypedParam(BigInt(1003), BigInt(5))],
  ["pow_moduleBig", () => pow_moduleBig()],
  ["mod_typedParam", () => mod_typedParam(BigInt(1003), BigInt(5))],
  ["mod_untypedParam", () => mod_untypedParam(BigInt(1003), BigInt(5))],
  ["mod_moduleBig", () => mod_moduleBig()],
  ["and_closure", () => and_closure(BigInt(1003), BigInt(5))],
  ["or_closure", () => or_closure(BigInt(1003), BigInt(5))],
  ["xor_closure", () => xor_closure(BigInt(1003), BigInt(5))],
  ["shl_closure", () => shl_closure(BigInt(1003), BigInt(5))],
  ["shr_closure", () => shr_closure(BigInt(1003), BigInt(5))],
  ["bitnot_untypedParam", () => bitnot_untypedParam(BigInt(1003))],
  ["bitnot_typedParam", () => bitnot_typedParam(BigInt(1003))],
  ["bitnot_moduleBig", () => bitnot_moduleBig()],
];
for (const [name, run] of cases) {
  try {
    console.log(name, run());
  } catch (e) {
    console.log(name, "threw", (e as Error).constructor.name, (e as Error).message);
  }
}

// Each section runs on its own so one failure cannot hide the next.
function section(name: string, run: () => void): void {
  try {
    run();
  } catch (e) {
    console.log(name, "threw", (e as Error).constructor.name, (e as Error).message);
  }
}

// Module-scope bindings take the module-global path, not a function local.
const MOD_AND = A_BIG & B_BIG;
const MOD_XOR = A_BIG ^ B_BIG;
let MOD_SHL = A_BIG << B_BIG;
console.log("module scope", show(MOD_AND), show(MOD_XOR), show(MOD_SHL), Number(A_BIG >> B_BIG));

// Compound assignment was already correct; keep it that way.
function compound(a, b): string {
  let x = a;
  x &= b;
  let y = a;
  y <<= b;
  let z = a;
  z ^= b;
  return show(x) + " " + show(y) + " " + show(z);
}
section("compound", () => console.log("compound", compound(BigInt(1003), BigInt(5))));

// `Number(n & M)` with a non-literal mask — @noble/curves `curve.js`.
const M32 = BigInt(2 ** 32 - 1);
function numberOfMask(n): number {
  return Number(n & M32);
}
function numberOfMaskTyped(n: bigint): number {
  return Number(n & M32);
}
section("Number(n & M)", () => {
  console.log(
    "Number(n & M)",
    typeof numberOfMask(BigInt("0x1234567890abcdef")),
    numberOfMask(BigInt("0x1234567890abcdef")),
    numberOfMaskTyped(BigInt("0xfedcba9876543210")),
  );
});

// @noble/curves weierstrass.js: `_2n << (c1 - _1n - _1n)` then `* _2n`.
function sqrtRatioPrelude(q): string {
  let l = _0n;
  for (let o = q - _1n; o % _2n === _0n; o /= _2n) l += _1n;
  const c1 = l;
  const _2n_pow_c1_1 = _2n << (c1 - _1n - _1n);
  const _2n_pow_c1 = _2n_pow_c1_1 * _2n;
  const c2 = (q - _1n) / _2n_pow_c1;
  return [c1, _2n_pow_c1_1, _2n_pow_c1, c2].map(String).join(" ");
}
section("sqrtRatio", () => {
  console.log("sqrtRatio", sqrtRatioPrelude(BigInt("0xffffffff00000001000000000000000000000000ffffffffffffffffffffffff")));
});

// @noble/curves: `Number(q.y & _1n)` on a point-like object.
function yParity(q): number {
  return Number(q.y & _1n);
}
section("parity", () => console.log("parity", yParity({ y: BigInt(12345) }), yParity({ y: BigInt(12344) })));

// @noble/hashes `_u64.ts`: fromBig / split / toBig round trip.
const U32_MASK64 = BigInt(2 ** 32 - 1);
const _32n = BigInt(32);
function fromBig(n: bigint, le = false): { h: number; l: number } {
  if (le) return { h: Number(n & U32_MASK64), l: Number((n >> _32n) & U32_MASK64) };
  return { h: Number((n >> _32n) & U32_MASK64) | 0, l: Number(n & U32_MASK64) | 0 };
}
function split(lst: bigint[], le = false): Uint32Array[] {
  const len = lst.length;
  const Ah = new Uint32Array(len);
  const Al = new Uint32Array(len);
  for (let i = 0; i < len; i++) {
    const { h, l } = fromBig(lst[i], le);
    [Ah[i], Al[i]] = [h, l];
  }
  return [Ah, Al];
}
const toBig = (h: number, l: number): bigint => (BigInt(h >>> 0) << _32n) | BigInt(l >>> 0);
section("u64", () => {
  const K512 = [
    "0x428a2f98d728ae22", "0x7137449123ef65cd", "0xb5c0fbcfec4d3b2f", "0xe9b5dba58189dbbc",
    "0x3956c25bf348b538", "0x59f111f1b605d019", "0x923f82a4af194f9b", "0xab1c5ed5da6d8118",
    "0xffffffffffffffff", "0x0000000000000000", "0x8000000000000001",
  ].map((n) => BigInt(n));
  const [Kh, Kl] = split(K512);
  const single = fromBig(BigInt("0x428a2f98d728ae22"));
  console.log("fromBig", single.h, single.l);
  let roundTrip = true;
  for (let i = 0; i < K512.length; i++) {
    const back = toBig(Kh[i], Kl[i]);
    if (back !== K512[i]) roundTrip = false;
    console.log("split", i, Kh[i], Kl[i], back.toString(16));
  }
  const le = fromBig(BigInt("0x0123456789abcdef"), true);
  console.log("fromBig le", le.h, le.l, "roundTrip", roundTrip);
});

// Mixed BigInt/Number operands must still throw.
function mixed(a, b): string {
  try {
    const x = a & b;
    return "no throw " + show(x);
  } catch (e) {
    return (e as Error).constructor.name;
  }
}
section("mixed", () => console.log("mixed", mixed(BigInt(3), 1), mixed(1, BigInt(3)), mixed(BigInt(3), BigInt(1))));

// Number controls: the int32 fast path must keep ToInt32 semantics.
function numUntyped(a, b): string {
  const x = a & b;
  const y = a << b;
  const z = a ^ b;
  return [x, y, z, Number(a | b), typeof (a >> b)].join(" ");
}
function numTyped(a: number, b: number): string {
  const x = a & b;
  const y = a << b;
  const z = (a ^ b) >>> 0;
  const w = a >> 1;
  return [x, y, z, w, Number(a | b)].join(" ");
}
section("number", () => {
  console.log("number untyped", numUntyped(1003, 5), numUntyped(-1, 31), numUntyped(0x7fffffff, 1));
  console.log("number typed", numTyped(1003, 5), numTyped(-5, 30), numTyped(4294967295, 3));
  let h = 0x811c9dc5 | 0;
  for (let i = 0; i < 1000; i++) {
    h ^= i & 0xff;
    h = Math.imul(h, 16777619);
    const t = (h << 5) | (h >>> 27);
    h = (h + t) | 0;
  }
  console.log("number hash", h, h >>> 0);
});
