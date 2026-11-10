// `p` is `a * b`, which is `-0` for a = 0 and b = -1; its uses are an int32
// compare, `p & 7` and `1 / p`. The division observes the sign of a zero, so
// the facts keep `p` a Number and `1 / p` is -Infinity (−0 tolerance, PR D).
// Under PERRY_FACTS_SABOTAGE=zerosign the division is no sign-observing use:
// `p` is I32 by tolerance, its slot holds 0 and `1 / p` would be Infinity.
// The sign witness at the division must report the false claim.
const n = process.argv.length > 100 ? 1 : -1;
let out = "";
for (let i = 0; i < 3; i++) {
  const a = i & 1;
  const p = a * n;
  out += (p === 0 ? String(1 / p) : String(p & 7)) + " ";
}
console.log(out);
