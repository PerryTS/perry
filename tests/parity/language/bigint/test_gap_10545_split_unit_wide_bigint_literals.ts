// #10545: BigInt literals wider than 64 bits lost their high bits in a module
// compiled as more than one codegen unit (the in-process IR reader built every
// `i128` operand from its low 64-bit word). The gap harness compiles single
// units, so this file pins the semantics; the split-module witness is
// crates/perry/tests/issue_10545_split_unit_wide_bigint_literals.rs.

// The issue repro.
function f() { const x = 2n ** 70n; return x === 1180591620717411303424n; }
function g() { return 1180591620717411303424n; }
console.log(f(), g() === 2n ** 70n, String(g()));
const small = 12345678901234567890n;
const neg = -98765432109876543210987654321n;
console.log(String(small), String(neg), 2n ** 64n === 18446744073709551616n);

// Around the 64-bit word boundary (these always survived; kept as controls).
const words = [
  9223372036854775807n,
  -9223372036854775808n,
  18446744073709551615n,
  18446744073709551616n,
  -18446744073709551616n,
  -18446744073709551617n,
];
console.log(words.map(String).join(" "));
console.log(words[3] === words[2] + 1n, words[5] === -(2n ** 64n) - 1n);

// Up to the i128 edge, in every radix a literal can be written in.
const edges = [
  170141183460469231731687303715884105727n,
  -170141183460469231731687303715884105727n,
  0x1ffffffffffffffffn,
  0o3777777777777777777777n,
  0b11111111111111111111111111111111111111111111111111111111111111111n,
  -0x10000000000000000n,
];
console.log(edges.map(String).join(" "));
console.log(edges[0] === 2n ** 127n - 1n, edges[1] === -(2n ** 127n) + 1n);
console.log(edges[2] === 2n ** 65n - 1n, edges[3] === 2n ** 65n - 1n, edges[4] === 2n ** 65n - 1n);

// Past i128: these take the string-materialized path.
const P = 0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffefffffc2fn;
const N = 0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141n;
console.log(P === 2n ** 256n - 2n ** 32n - 977n, String(P % 1000000007n), String(N % 998244353n));

// Wide literals as operands, in closures, containers, class fields and defaults.
const mul = (a: bigint) => a * 1180591620717411303424n;
console.log(String(mul(3n)), String(mul(-5n) / 36893488147419103232n));
const obj = { lo: 36893488147419103232n, hi: -73786976294838206464n };
console.log(String(obj.lo + obj.hi), obj.lo > 18446744073709551615n, obj.hi < -18446744073709551616n);
class Field {
  static readonly Q = 340282366920938463463374607431768211455n;
  mask(v: bigint, m = 1267650600228229401496703205375n) { return v & m; }
}
console.log(String(Field.Q), Field.Q === 2n ** 128n - 1n, String(new Field().mask(-1n)));
console.log(1n << 100n === 1267650600228229401496703205376n, BigInt.asUintN(96, -1n) === 79228162514264337593543950335n);
let acc = 0n;
for (let i = 0; i < 5; i++) acc += 1180591620717411303424n - 1n;
console.log(String(acc), typeof 1180591620717411303424n);
