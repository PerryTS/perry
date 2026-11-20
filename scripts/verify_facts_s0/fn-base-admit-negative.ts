// `probe` reads the board through `at`, then calls `grow`, which pushes onto
// the board through an alias past its capacity and moves the storage, then
// reads again. The call ends the held region, so the function-level base
// (N2) starts after it. Under PERRY_FACTS_SABOTAGE=fnbaseadm the whole body
// is admitted and the base is installed before the call; the first read
// after the growth must report that the held base is not the live head.
class Board {
  board: number[] = new Array(16).fill(1);
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  grow(n: number): void {
    const alias = this.board;
    for (let k = 0; k < n; k++) alias.push(k + 2);
  }
  probe(): number {
    let s = this.at(1) + this.at(2);
    this.grow(64);
    s += this.at(3) + this.at(5);
    return s;
  }
}
const b = new Board();
// A birth right behind the board keeps it from growing in place.
const blocker = new Board();
console.log(b.probe(), b.board.length, blocker.at(3));
