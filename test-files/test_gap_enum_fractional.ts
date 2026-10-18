// Numeric enums retain JS numbers, including computed non-finite values.
enum E { A = 1.5, B = -0.25, C = A * 2, D = 1e21, E = 0.1 + 0.2, Negative = -17.75, Zero = -0, Tiny = 1e-7, Big = 1e20, Inf = 1 / 0, NegInf = -1 / 0, Nan = 0 / 0 }
console.log(E.A, E.B, E.C, E.D, E.E, E.Negative);
console.log(Object.is(E.Zero, -0), Object.is((E as any)["Zero"], -0));
console.log(E[1.5], E["1.5"], E[-0.25], E[3], E[1e21], E[0.1 + 0.2]);
console.log(E[0], E["1e-7"], E["100000000000000000000"], E["Infinity"], E["-Infinity"], E["NaN"]);
console.log(Object.keys(E).join(","));
const enum Inline { A = 1.5, B = -0.25, C = A * 2, Z = -0, D = 1e21 }
console.log(Inline.A, Inline.B, Inline.C, Object.is(Inline.Z, -0), Inline.D);
enum M { A = -0.5 }
enum M { B = 2.25, C = M.A * 2, D = A * 3 }
console.log(M.A, M.B, M.C, M[-0.5], M[2.25], M[-1], M.D, Object.keys(M).join(","));
enum Bits { A = 4294967297.5 | 0, B = -1 >>> 0, C = 1 << 32.5, D = 2 ** 3, R = 5.5 % 2, Next }
console.log(Bits.A, Bits.B, Bits.C, Bits.D, Bits.R, Bits.Next);
// esbuild/tsc-style image births share the same number representation.
var Image = ((P: any) => { P[P["A"] = 1.5] = "A"; P[P["Z"] = -0] = "Z"; P[P["L"] = 1e21] = "L"; P[P["T"] = 1e-7] = "T"; return P; })(Image || {});
console.log(Image.A, Object.is(Image.Z, -0), Image["1.5"], Image["1e+21"], Image["1e-7"], Object.keys(Image).join(","));

let zero = 0;
enum Computed { Z = -zero, A = zero + 1.5, Inf = 1 / zero, Nan = zero / zero, Next = Computed.A + 1 }
console.log(Object.is(Computed.Z, -0), Object.is(Computed["Z"], -0), Computed.A, Computed.Inf, Number.isNaN(Computed.Nan), Computed.Next);
zero = 10;
console.log(Object.is(Computed.Z, -0), Computed.A, Computed.Next, Computed["0"], Computed["1.5"], Computed["Infinity"], Computed["NaN"], Object.keys(Computed).join(","));

enum ConstantZero { Z = -0, Reciprocal = 1 / Z }
console.log(Object.is(ConstantZero.Z, -0), ConstantZero.Reciprocal);
