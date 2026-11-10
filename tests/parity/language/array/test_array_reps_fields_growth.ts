// DESIGN-p4l1 §5.4 fixture 14: a loop reads `this.f[i]` and pushes to
// `this.f`, so the storage moves while the loop runs; and an index store
// past the capacity grows a field-held array that is read before and
// after.

class Queue {
  items: number[] = [1, 2, 3];
  at(i: number): number {
    return this.items[i] ?? -1;
  }
  run(n: number): number {
    let s = 0;
    for (let i = 0; i < n; i++) {
      s += this.items[i]! + this.at(i);
      this.items.push(this.items[i]! * 2 + 1);
    }
    return s % 1000000007;
  }
}
const q = new Queue();
console.log(q.run(40) > 0, q.items.length, q.at(0), q.at(42), q.at(43), q.at(-1));

class Board {
  board: number[] = new Array(128).fill(0);
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  sum(): number {
    let s = 0;
    for (let i = 0; i < 128; i++) s += this.at(i);
    return s;
  }
}
const b = new Board();
for (let i = 0; i < 128; i++) b.board[i] = i;
const before = b.sum();
// One store far past the capacity: the storage is reallocated.
b.board[5000] = 7;
let after = 0;
for (let r = 0; r < 100; r++) after += b.sum();
console.log(before, after, b.at(5), b.at(127), b.at(128), b.at(5000), b.board.length);
