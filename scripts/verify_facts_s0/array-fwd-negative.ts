// `grow()` pushes 100 elements onto a 3-element literal, past its capacity:
// the runtime reallocates the storage and forwards the old header. Under
// PERRY_FACTS_SABOTAGE=arrfwd an append keeps the length interval, so the
// facts claim `K` keeps `lo == hi == 3` (storage that never moves), and the
// witness of the proven read `K[i]` must report the false proof.
const K = [1, 2, 3];
function grow(): void {
  for (let i = 0; i < 100; i++) K.push(i);
}
function at(i: number): number {
  return K[i] ?? 0;
}
grow();
console.log(at(1) + at(2), K.length);
