// `at` is a field element read, and `board` is born dense with 16
// elements. `probe` calls it with keys that leave the floor (`ep - 16` is
// negative, `ep + 16` is past the end), whose value is the `?? 0`.
// Under PERRY_FACTS_SABOTAGE=arrb1 the facts claim every such call's key
// is in the floor, so the call loads the slot with no bound. The element
// tier's witness must report the false proof at the key.
class Board {
  board: number[] = new Array(16).fill(1);
  ep: number = process.argv.length > 100 ? 5 : 3;
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  probe(): number {
    const ep = this.ep;
    return this.at(ep) * 100 + this.at(ep - 16) * 10 + this.at(ep + 16);
  }
}
const b = new Board();
let s = 0;
for (let i = 0; i < 3; i++) s += b.probe();
console.log(s);
