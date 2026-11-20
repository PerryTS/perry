// N2 (DESIGN-n2-fn-bases.md): a method that holds a fresh, young board's
// element base from its first read to its end, with a polled loop in the
// held suffix, called from an allocating loop. Under a seeded GC schedule
// (PERRY_GC_SCHEDULE_SEED with the instrumented runtime) collections move
// the board at the suffix's poll; the poll must re-derive the base from
// `this`'s root, and the reads after the loop must see the moved board.

const STEPS = [1, 3, 5, 7];

class Board {
  board: number[] = new Array(128).fill(0);
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  scan(from: number): number {
    let s = this.at(from) + this.at(from + 1);
    for (const d of STEPS) s += this.at(from + d);
    let i = from;
    while (i < 128) {
      s += this.at(i);
      i += 1;
    }
    return s + this.at(from) * 3 + this.at(127);
  }
}

let keep: number[][] = [];
let total = 0;
for (let round = 0; round < 50000; round++) {
  const b = new Board();
  b.board[round & 127] = round & 15;
  b.board[(round * 5) & 127] = 3;
  keep.push([round, round + 1]);
  if (keep.length > 64) keep = [];
  total += b.scan(round & 63);
}
console.log(total, keep.length);
