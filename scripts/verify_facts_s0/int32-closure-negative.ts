// The same false int32 claim (PERRY_FACTS_SABOTAGE=range, `i + 3000000000`
// claimed in [0, 7]) on a `const` inside a closure body and inside a static
// method: both are bodies without a proven-`let` set, where the facts' `I32`
// still selects the canonical i32 slot. Each call must be reported.
const RUN = Math.random() < 2 ? 1 : 0;
const h = (i: number): number => {
  let t = 0;
  for (let q = 0; q < RUN; q++) {
    const d = i + 3000000000;
    if (d === 7) t = 1;
    t = t * 2 + d * 0.5;
  }
  return t;
};
class M {
  static g(i: number): number {
    let t = 0;
    for (let q = 0; q < RUN; q++) {
      const d = i + 3000000000;
      if (d === 7) t = 1;
      t = t * 2 + d * 0.5;
    }
    return t;
  }
}
const outv: number[] = [];
for (let i = 0; i < 4 * RUN; i++) outv.push(process.argv.length > 100 ? M.g(i) : h(i));
for (let i = 0; i < 4 * RUN; i++) outv.push(process.argv.length > 100 ? h(i) : M.g(i));
console.log(outv.join(","));
