// Number.prototype.toString(radix) prints in the given base for literals, locals,
// negatives, fractions and a radix computed at run time; no radix and 10 print decimal.
const n = 255;
const r: number = Number("16");
console.log((255).toString(16), (8).toString(2), (35).toString(36), (-255).toString(16));
console.log((0.5).toString(2), n.toString(r), n.toString(), n.toString(10));
const big = 2 ** 40;
console.log(big.toString(32), (1e21).toString(16));
