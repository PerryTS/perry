// P2 (DESIGN-p2 §5, R2): only an escape route (`f.call` from a callback)
// passes `f` a fraction; its typed call passes integers. Under
// PERRY_FACTS_SABOTAGE=signature-escape the facts claim I32 for the
// parameter: the boxed-entry witness must report it.
// A loop bound the compiler cannot fold keeps each call site one HIR call
// (a static loop is unrolled, and its copies share the site's offset).
const RUN = Math.random() < 2 ? 1 : 0;
function f(x: number): number {
  let r = 1;
  for (let k = 0; k < 2; k++) r += x;
  return r;
}
let s = 0;
for (let i = 0; i < 4 * RUN; i++) s += f(i);
[1.5].forEach((v) => {
  s += f.call(null, v);
});
console.log(s);
