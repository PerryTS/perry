// DESIGN-p4l4 §1.4, §5.3 (VR-4b): record arrays. An undo stack held in a
// class field and one held in a local: the births (a field initializer, a
// store, a `let`), push of a call's result, pop (also on empty), length, an
// index read (also out of bounds, fractional, negative) and for-of. Garbage
// is allocated between the operations, so collections land while the stacks
// grow, are read and are replaced.
// vr-expect: move,piece,captured,rights,ep,flags ADMITTED
interface Undo {
  move: number;
  piece: number;
  captured: number;
  rights: number;
  ep: number;
  flags: boolean;
}
class Game {
  private played: Undo[] = [];
  board: number[] = new Array(64).fill(0);
  junk: string[] = [];
  reset(): void {
    this.played = [];
  }
  private make(m: number): Undo {
    const undo: Undo = {
      move: m,
      piece: this.board[m & 63],
      captured: this.board[(m >> 6) & 63],
      rights: m & 15,
      ep: ((m * 7) % 64) - 1,
      flags: m % 3 === 0,
    };
    this.board[(m >> 6) & 63] = undo.piece;
    this.board[m & 63] = 0;
    return undo;
  }
  private unmake(u: Undo): number {
    this.board[u.move & 63] = u.piece;
    this.board[(u.move >> 6) & 63] = u.captured;
    return u.rights + u.ep + 2 + (u.flags ? 1 : 0);
  }
  play(m: number): number {
    return this.played.push(this.make(m));
  }
  back(): number {
    const u = this.played.pop();
    if (u === undefined) return -1;
    return this.unmake(u);
  }
  depth(): number {
    return this.played.length;
  }
  history(): number {
    let h = 0;
    for (const u of this.played) h = (h * 31 + u.move + (u.flags ? 7 : 0)) % 1000003;
    return h;
  }
  at(i: number): number {
    const u = this.played[i];
    return u === undefined ? -7 : u.move * 2 + u.captured;
  }
  line(n: number): number {
    const undos: Undo[] = [];
    for (let i = 0; i < n; i++) {
      undos.push(this.make((i * 131 + 17) & 4095));
      if (i % 16 === 0) this.junk.push("j" + i);
    }
    let s = undos.length;
    for (let i = undos.length - 1; i >= 0; i--) {
      const u = undos[i];
      s = (s * 17 + this.unmake(u)) % 1000003;
    }
    return s;
  }
}
const g = new Game();
for (let i = 0; i < 64; i++) g.board[i] = i % 7;
console.log(g.back(), g.depth(), g.at(0), g.history());
let check = 0;
for (let i = 0; i < 3000; i++) {
  check = (check * 13 + g.play((i * 37 + 5) & 4095)) % 1000003;
  if (i % 3 === 0) check = (check + g.back() + 100) % 1000003;
  if (i % 97 === 0) {
    g.junk = [];
    check = (check + g.history()) % 1000003;
  }
  if (i % 500 === 499) check = (check + g.line(40 + (i % 7))) % 1000003;
}
console.log(check, g.depth(), g.at(0), g.at(5), g.at(g.depth() - 1), g.at(g.depth()), g.at(-1), g.at(1.5), g.history());
let drained = 0;
while (g.back() >= 0) drained++;
console.log(drained, g.depth(), g.back(), g.history());
g.play(77);
g.play(78);
console.log(g.depth(), g.at(1));
g.reset();
console.log(g.depth(), g.history(), g.at(0), g.board.reduce((a, b) => a + b, 0));
