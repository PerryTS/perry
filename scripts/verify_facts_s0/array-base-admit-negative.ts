// `scan`'s loop reads the board through `at` on every iteration, but it
// also calls `grow`, which pushes past the capacity and moves the storage,
// so no loop may hold the board's element base (PR-D admission). Under
// PERRY_FACTS_SABOTAGE=arrbaseadm the loop is admitted anyway; the read
// after the first growth must report that the held base is not the live
// head.
class Board {
  board: number[] = new Array(16).fill(1);
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  grow(n: number): void {
    for (let k = 0; k < n; k++) this.board.push(k);
  }
  scan(n: number): number {
    let s = 0;
    for (let i = 0; i < n; i++) {
      s += this.at(i);
      this.grow(64);
    }
    return s;
  }
}
const b = new Board();
// A birth right behind the board keeps it from growing in place.
const blocker = new Board();
console.log(b.scan(8), b.board.length, blocker.at(3));
