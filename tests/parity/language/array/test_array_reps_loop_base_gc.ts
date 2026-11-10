// PR-D (DESIGN-p4l1 §4.3): a loop that holds a field-held board's element
// base, called from an allocating loop, so collections run at the held
// loop's poll and between its runs. Run under PERRY_GC_MOVING_LOOP_POLLS=1
// and a seeded PERRY_GC_STRESS, the poll's refresh must re-derive the base
// from `this`'s root every time a collection moves the board.

class Board {
  board: number[] = new Array(128).fill(0);
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  scan(from: number): number {
    let s = 0;
    let i = from;
    while (i < 128) {
      s += this.at(i);
      i += 1;
    }
    return s;
  }
}

let keep: number[][] = [];
let total = 0;
for (let round = 0; round < 100000; round++) {
  const b = new Board();
  b.board[round & 127] = round & 15;
  if ((round & 1023) === 0) b.board[300] = 1;
  keep.push([round, round + 1]);
  if (keep.length > 64) keep = [];
  total += b.scan(round & 63);
}
console.log(total, keep.length);
