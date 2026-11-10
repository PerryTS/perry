// `at` is a field element read, and `board` is born dense with 116
// elements. `sum` reads the keys `r * 16 + f` for `r, f` in `[0, 7]`: the
// sum `[0, 119]` leaves the floor, and the reads past it are the `?? 0`.
// Under PERRY_FACTS_SABOTAGE=exactrule a sum's interval is the hull of its
// operands (`r * 16` is `[0, 112]`), so the facts claim the key is in the
// floor and the call loads the slot with no bound. The element tier's witness must report the
// false proof at the key.
class Board {
  board: number[] = new Array(116).fill(1);
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  sum(): number {
    let s = 0;
    for (let r = 0; r < 8; r++) for (let f = 0; f < 8; f++) s += this.at(r * 16 + f);
    return s;
  }
}
const b = new Board();
console.log(b.sum());
