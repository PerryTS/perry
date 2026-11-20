// I32 at rest, PR A: Number fields get the representation of their nodes
// (I32 for int32 values with int demand, F64 otherwise), and demand reaches
// an element read behind `?? 0`. Negative controls that must stay double:
// -0, NaN, 2^31, 2^53, a `|0` wrap, `>>>` of a negative, a field that later
// holds a fraction, and objects printed, serialized and spread.

class Pos {
  hash = 0;
  rights = 15;
  king = -1;
  wide = 0;
  zero = 1;
  late = 0;
  board: number[] = new Array<number>(16).fill(0);
  step(k: number, n: number): number {
    this.hash ^= (k * 2654435761) | 0;
    this.rights &= ~(k & 3);
    this.king = (k & 7) - 1;
    this.wide = 2147483647 + (k & 3);
    this.zero = (k - k) * n;
    if (k === 37) this.late = k / 4;
    else this.late = k & 1;
    this.board[k & 15] = (k % 13) - 6;
    const p = this.board[(k * 5) & 15] ?? 0;
    return (p === 3 ? 1 : 0) + (this.king === 2 ? 1 : 0) + (this.hash & 1) + (this.rights >>> 0);
  }
}

const pos = new Pos();
const n = process.argv.length > 100 ? 1 : -1;
let s = 0;
for (let i = 0; i < 200; i++) s += pos.step(i, n);
console.log(s, pos.hash, pos.rights, pos.king, pos.wide, pos.late);
console.log(Object.is(pos.zero, -0), 1 / pos.zero, pos.zero === 0);
console.log(pos.hash | 0, pos.hash >>> 0, (pos.wide + 1) | 0, pos.wide >>> 0);
console.log(JSON.stringify({ h: pos.hash, r: pos.rights, k: pos.king, z: pos.zero }));
const copy = { ...pos };
console.log(copy.hash === pos.hash, copy.king, copy.board.length, Object.keys(copy).length);
console.log(pos.board.join(","));

// Past 2^53 and NaN in a field: F64, exact.
class Big {
  v = 0;
  nan = 0;
  set(k: number): void {
    this.v = 9007199254740992 + k * 2;
    this.nan = k > 2 ? NaN : k;
  }
}
const b = new Big();
for (let i = 0; i < 4; i++) b.set(i);
console.log(b.v, Number.isNaN(b.nan), b.v - 9007199254740992);
