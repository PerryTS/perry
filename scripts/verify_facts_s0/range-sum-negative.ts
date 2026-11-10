// `at` is a field element read, and `board` is born dense with 116
// elements. `probe` reads the key `ep + 112` with `ep` in `[3, 5]`: the sum
// `[115, 117]` leaves the floor, and the read past it is the `?? 0`.
// Under PERRY_FACTS_SABOTAGE=exactrule a sum's interval is the hull of its
// operands (`[3, 112]`), so the facts claim the key is in the floor and the
// call loads the slot with no bound. The element tier's witness must report
// the false proof at the key.
class Board {
  board: number[] = new Array(116).fill(1);
  ep: number = process.argv.length > 100 ? 3 : 5;
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  probe(): number {
    const ep = this.ep;
    return this.at(ep) * 10 + this.at(ep + 112);
  }
}
const b = new Board();
let s = 0;
for (let i = 0; i < 3; i++) s += b.probe();
console.log(s);
