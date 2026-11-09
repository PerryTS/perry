// P3 B1 (DESIGN-p3 rev 3.1 §5): `Math.round` is a declared-NUM builtin. A
// runtime built with PERRY_FACTS_SABOTAGE=canonnum returns an INT32-boxed word
// from it. The value is stored into an array, and the element read the facts
// prove a plain Number reaches a compare (C1), an arithmetic leaf (C2) and a
// Number parameter (C4): the canonical-number witness must report it (rc 86).
// The input is not a constant, so `Math.round` really runs.
function twice(x: number): number {
  return x * 2;
}
function run(seed: number): string {
  const vals: number[] = [];
  for (let i = 0; i < 4; i++) vals.push(Math.round(seed + i + 0.4));
  let hits = 0;
  let total = 0;
  for (let i = 0; i < vals.length; i++) {
    if (vals[i] === seed + i) hits++;
    total = total + vals[i] * 3;
    total += twice(vals[i]);
  }
  return hits + " " + total;
}
console.log(run(process.argv.length));
