// P3 B4b: a birth stores a proven Number into a raw f64 lane with no number
// test. `Math.round` is a declared-NUM builtin; a runtime built with
// PERRY_FACTS_SABOTAGE=canonnum returns an INT32-tagged word from it, and the
// canonical-number witness at the birth must report it (rc 86). The input is
// not a constant, so `Math.round` really runs; the objects escape into an
// Array, so they are born on the heap.
class P {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}
function run(seed: number): string {
  const kept: P[] = [];
  const lits: { a: number; b: number }[] = [];
  for (let i = 0; i < 4; i++) {
    kept.push(new P(Math.round(seed * 1.5 + i + 0.4), i + 0.5));
    lits.push({ a: Math.round(seed * 2.5 + 0.4), b: i * 0.25 });
  }
  let s = 0;
  for (let i = 0; i < 4; i++) s += kept[i].x + kept[i].y + lits[i].a + lits[i].b;
  return String(s);
}
console.log(run(process.argv.length));
