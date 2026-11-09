// P2 (DESIGN-p2 §5): `scale`'s typed call passes fractions, so its parameter
// is F64. Under PERRY_FACTS_SABOTAGE=signature the facts claim I32 for it:
// the typed-site witness of the signature must report it.
function scale(x: number): number {
  let r = 0;
  for (let k = 0; k < 2; k++) r += x;
  return r;
}
let s = 0;
for (let i = 0; i < 4; i++) s += scale(i + 0.5);
console.log(s);
