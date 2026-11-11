// DESIGN-p4l4 §3: a chess-like make/unmake over a played stack, a record
// array since the VR-4b intrinsics (push, pop).
// vr-expect: move,piece,captured,flags ADMITTED
class Game {
  played: { move: number; piece: number; captured: number; flags: boolean }[] = [];
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
  play(m: number) {
    this.played.push(this.make(m));
  }
  back() {
    const u = this.played.pop();
    if (u !== undefined) this.unmake(u);
  }
}
const g = new Game();
for (let i = 0; i < 64; i++) g.board[i] = i % 7;
let check = 0;
for (let i = 0; i < 200; i++) {
  g.play((i * 37) & 4095);
  if (i % 3 === 0) g.back();
  check = (check * 31 + g.board[i & 63]) | 0;
}
for (const u of g.played) check = (check + u.move) | 0;
console.log(check, g.played.length);
