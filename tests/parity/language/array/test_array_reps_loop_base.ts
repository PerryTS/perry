// DESIGN-p4l1 §5.4 fixture 15 (PR-D): the element base of a field-held
// board held across a loop. The board grows once (`b[200] = 1`), so its
// header is a forwarding stub; hot loops then read it inside a loop that
// holds the base (every iteration reads first: the install resolves the
// stub) and outside one (the read is conditional). Then the shapes the
// installer must refuse or serve differently: a read outside the call's
// floor (passed-pawn shape), a loop that runs zero times, a `continue`
// before the read, a loop that grows the board, a loop that replaces it,
// a do-while, two fields in one loop, and nested loops.

class Board {
  board: number[] = new Array(128).fill(0);
  other: number[] = new Array(16).fill(3);
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  far(sq: number): number {
    return this.other[sq] ?? -1;
  }
  // In a region: the read dominates the back edge.
  ray(from: number, step: number): number {
    let sq = from;
    let n = 0;
    while (sq >= 0 && sq < 128 && (sq & 0x88) === 0) {
      const p = this.at(sq);
      if (p !== 0) return n * 100 + p;
      n++;
      sq += step;
    }
    return n;
  }
  // Outside a region: some iterations skip the read.
  sparse(n: number): number {
    let s = 0;
    for (let i = 0; i < n; i++) {
      if ((i & 3) === 0) s += this.at(5);
    }
    return s;
  }
  hot(n: number): number {
    let s = 0;
    for (let i = 0; i < n; i++) s += this.at(5) + this.at(i & 127);
    return s;
  }
  // The key leaves the floor: the slot below it, the call's read above.
  passed(rank: number, file: number): number {
    let s = 0;
    for (let r = rank; r < 16; r++) {
      for (let f = Math.max(0, file - 1); f <= Math.min(7, file + 1); f++) {
        s += this.at(r * 16 + f);
      }
    }
    return s;
  }
  never(): number {
    let s = 0;
    let i = 10;
    while (i < 5) {
      s += this.at(i);
      i++;
    }
    return s;
  }
  skips(): number {
    let s = 0;
    for (let sq = 0; sq < 128; sq++) {
      if ((sq & 0x88) !== 0) {
        sq += 7;
        continue;
      }
      s += this.at(sq);
    }
    return s;
  }
  grows(n: number): number {
    let s = 0;
    for (let i = 0; i < n; i++) {
      s += this.at(i & 127);
      this.board.push(i);
    }
    return s;
  }
  replaces(n: number, next: number[]): number {
    let s = 0;
    for (let i = 0; i < n; i++) {
      s += this.at(i & 7);
      if (i === 2) this.board = next;
    }
    return s;
  }
  once(): number {
    let s = 0;
    let i = 0;
    do {
      s += this.at(i);
      i += 2;
    } while (i < 20);
    return s;
  }
  both(n: number): number {
    let s = 0;
    for (let i = 0; i < n; i++) s += this.at(i & 127) * 10 + this.far(i & 15);
    return s;
  }
  nested(n: number): number {
    let s = 0;
    for (let i = 0; i < n; i++) {
      s += this.at(i & 127);
      for (let j = 0; j < 3; j++) s += this.at((i + j) & 127);
    }
    return s;
  }
}

const b = new Board();
for (let i = 0; i < 128; i++) b.board[i] = (i * 7 + 3) % 5;
b.board[0] = 0;
console.log("before", b.ray(0, 1), b.hot(1000), b.sparse(1000), b.passed(6, 3));
// One store far past the capacity: the storage moves, the header forwards.
b.board[200] = 1;
console.log("length", b.board.length, b.at(200), b.at(199), b.at(5));
let total = 0;
for (let k = 0; k < 2000; k++) total += b.hot(64) + b.ray(k & 7, 16);
console.log("forwarded", total, b.sparse(4000));
console.log("passed", b.passed(0, 0), b.passed(6, 7), b.passed(12, 4));
console.log("never", b.never(), "skips", b.skips(), "once", b.once());
console.log("both", b.both(500), "nested", b.nested(300));
const c = new Board();
for (let i = 0; i < 128; i++) c.board[i] = i & 3;
console.log("grows", c.grows(300), c.board.length, c.at(127), c.at(128));
const d = new Board();
const next = new Array(128).fill(9);
console.log("replaces", d.replaces(6, next), d.at(0));
