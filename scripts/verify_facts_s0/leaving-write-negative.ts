// `at` is a field element read, and `board` is born dense with 16
// elements. In `walk` the counter write `q += 20` is in a branch that falls
// through to the read, so the read's key reaches past the floor (its value
// is the `?? 0`). Under PERRY_FACTS_SABOTAGE=leavingwrite every branch's
// writes are dropped from the reads after it, so the facts claim the key is
// the loop test's `[0, 15]`, in the floor, and the call loads the slot with
// no bound. The element tier's witness must report the false proof at the
// key.
class Board {
  board: number[] = new Array(16).fill(1);
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  walk(n: number): number {
    let s = 0;
    for (let q = 0; q < 16; q++) {
      if (q === n) {
        q += 20;
      }
      s += this.at(q);
    }
    return s;
  }
}
const b = new Board();
console.log(b.walk(process.argv.length > 100 ? 99 : 5));
