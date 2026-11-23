// `at` is a field element read, and `board` is born dense with 16
// elements. In `walk` the guard `q < 16` narrows `q`, and the write
// `q += 20` is in a branch that falls through to the read, so the read's key
// reaches past the floor (its value is the `?? 0`). Under
// PERRY_FACTS_SABOTAGE=leavingwrite every branch's writes are dropped from
// the reads after it, so the facts claim the key is the guard's `[3, 15]`,
// in the floor, and the call loads the slot with no bound. The element
// tier's witness must report the false proof at the key.
class Board {
  board: number[] = new Array(16).fill(1);
  ep: number = process.argv.length > 100 ? 3 : 5;
  far: boolean = process.argv.length < 100;
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  walk(): number {
    let q = this.ep;
    if (q < 16) {
      if (this.far) {
        q += 20;
      }
      return this.at(q);
    }
    return -1;
  }
}
const b = new Board();
let s = 0;
for (let i = 0; i < 3; i++) s += b.walk() + 1;
console.log(s);
