// `put(5)` stores past K's birth length, so K's length moves to 6. Under
// PERRY_FACTS_SABOTAGE=arrlen every index store counts as one inside the
// birth length, so the facts claim `K.length` stays 3: the witness of the
// for-of bound must report it.
const K = [1, 2, 3];
function put(i: number): void {
  K[i] = 0;
}
function total(): number {
  let s = 0;
  for (const x of K) s += x ?? 0;
  return s;
}
put(5);
console.log(total());
