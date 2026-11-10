// `scan`'s loop reads the board through `at` on every iteration, so it
// holds the board's element base (PR-D). The board grew past its capacity
// first, so the field holds a forwarding stub. Under
// PERRY_FACTS_SABOTAGE=arrbase the install keeps the stub's handle instead
// of resolving it to the live head; the install's witness must report the
// forwarded head.
class Board {
  board: number[] = new Array(16).fill(1);
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  scan(): number {
    let s = 0;
    let i = 0;
    while (i < 16) {
      s += this.at(i);
      i++;
    }
    return s;
  }
}
const b = new Board();
b.board[process.argv.length > 100 ? 7 : 40] = 2;
console.log(b.scan(), b.at(40), b.board.length);
