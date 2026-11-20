// P2 (DESIGN-p2 §5, T1): one caller passes `pick` -1, so its I32
// parameter's interval starts below 0. Under
// PERRY_FACTS_SABOTAGE=signature-range the facts claim [0, 127]: the
// interval witness of the signature must report it.
// A loop bound the compiler cannot fold keeps each call site one HIR call
// (a static loop is unrolled, and its copies share the site's offset).
const RUN = Math.random() < 2 ? 1 : 0;
function pick(i: number): number {
  let r = 0;
  for (let k = 0; k < 3; k++) r = (r + i) | 0;
  return r;
}
let s = 0;
for (let i = 0; i < 8 * RUN; i++) s += pick(i);
s += pick(-1);
console.log(s);
