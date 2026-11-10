// `probe` walks `DIRS` with for-of and reads the board in the walk's body.
// `DIRS` has an accessor element, so N3 does not decide the walk: the
// lowering keeps the walk's entry and the iterator branch, whose element
// read runs the getter, which pushes onto the board through an alias and
// moves its storage. The walk is not admitted, so no function-level base
// (N2) is held across it. Under PERRY_FACTS_SABOTAGE=fnbasewalk any
// walk-shaped `if` counts as its body only; the read after the getter ran
// must report that the held base is not the live head.
const DIRS: number[] = [1, 2, 3];
let alias: number[] = [];
Object.defineProperty(DIRS, 1, {
  get() {
    for (let k = 0; k < 64; k++) alias.push(k);
    return 2;
  },
});
class Board {
  board: number[] = new Array(16).fill(1);
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  probe(): number {
    let s = 0;
    for (const d of DIRS) {
      s += this.at(d) + this.at(d + 4);
    }
    return s;
  }
}
const b = new Board();
// A birth right behind the board keeps it from growing in place.
const blocker = new Board();
alias = b.board;
console.log(b.probe(), b.board.length, blocker.at(3));
