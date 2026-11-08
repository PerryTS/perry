// `put(5)` stores past K's birth length, so K's length moves to 6. Under
// PERRY_FACTS_SABOTAGE=arrlen every index store counts as one inside the
// birth length: the facts claim K keeps its 3 elements for good, so `K[i]`
// is a proven element read, and its witness must report the false proof.
const K = [1, 2, 3];
function put(i: number): void {
  K[i] = 0;
}
function at(i: number): number {
  return K[i] ?? 0;
}
put(5);
console.log(at(1) + at(2));
