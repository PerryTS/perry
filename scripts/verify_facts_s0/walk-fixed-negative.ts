// `A` is a literal of Numbers that `walk` pops while its for-of runs,
// so it is neither immutable nor of a fixed length: the loop reads the
// length at every step and stops after the third element.
// Under PERRY_FACTS_SABOTAGE=walkfixed any Array of Numbers counts as an
// indexed receiver (immutable, dense, of fixed length), so the walk reads
// the length once and takes index 3 as a present element. The read's
// witness must report the key that is no longer below the length.
const A = [1, 2, 3, 4, 5, 6];
function walk(cut: number): number {
  let s = 0;
  for (const v of A) {
    s += v;
    if (v === cut) {
      A.pop();
      A.pop();
      A.pop();
    }
  }
  return s;
}
const z = process.argv.length > 100 ? 1 : 0;
console.log(walk(2 + z), A.length);
