// VR-3b (DESIGN-p4l4 §5.2): a record crosses a typed call as its lanes.
// A record parameter is its field locals, a record result is returned by
// value, and neither side births an object. The helpers call themselves or
// loop so that the inliner leaves the calls in place.
// vr-expect: sa,sb,sc ADMITTED
// vr-expect: move,piece,captured,flags ADMITTED
function mk(n: number, depth: number): { sa: number; sb: number; sc: boolean } {
  if (depth > 0) return mk(n + 1, depth - 1);
  return { sa: n, sb: n * 2, sc: n % 2 === 0 };
}
function sum(r: { sa: number; sb: number; sc: boolean }, k: number): number {
  let s = 0;
  for (let i = 0; i < k; i++) s += r.sc ? r.sa : r.sb;
  return s;
}
function pick(r: { sa: number; sb: number; sc: boolean } | undefined, depth: number): number {
  if (depth > 0) return pick(r, depth - 1) + 1;
  return r === undefined ? -1 : r.sa + r.sb;
}
let total = 0;
for (let i = 0; i < 6; i++) {
  const r = mk(i, i % 3);
  total += sum(r, 3) + pick(r, 2) + pick(i % 2 === 0 ? r : undefined, 1) + pick(undefined, 0);
  // A field of a call's result, and a result at a boundary (a copy).
  total += mk(i, 1).sb;
  console.log(mk(i, 0), r.sc);
}
console.log(total);
// A chess-like make/unmake pair: the record is born in `make`, returned by
// value and passed to `unmake` by value.
class Game {
  board: number[] = new Array(64).fill(0);
  private make(m: number) {
    const undo = { move: m, piece: this.board[m & 63], captured: this.board[(m >> 6) & 63], flags: m > 100 };
    this.board[(m >> 6) & 63] = undo.piece;
    this.board[m & 63] = 0;
    return undo;
  }
  private unmake(u: { move: number; piece: number; captured: number; flags: boolean }) {
    this.board[u.move & 63] = u.piece;
    this.board[(u.move >> 6) & 63] = u.captured;
  }
  step(m: number): number {
    const u = this.make(m);
    const seen = this.board[(m >> 6) & 63] + (u.flags ? 1 : 0);
    this.unmake(u);
    return seen + u.captured;
  }
}
const g = new Game();
for (let i = 0; i < 64; i++) g.board[i] = i % 7;
let check = 0;
for (let i = 0; i < 200; i++) {
  check = (check * 31 + g.step((i * 37) & 4095)) | 0;
}
console.log(check, g.board[5], g.board[40]);
