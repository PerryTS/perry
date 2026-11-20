// b1: a method call proven to be a field element read lowers as that read.
// The read must equal the call everywhere: holes, null, out of range, 0
// and NaN (which keep their value under `??`); and the shapes that keep the
// call (an override, an argument with effects, an extra argument) still
// dispatch.
class Board {
  board: (number | null)[] = [0, 1, NaN, null, 4, 5, 6, 7];
  at(i: number): number | null { return this.board[i] ?? -1; }
  swap(): number { this.board = [70, 71, 72, 73]; return 1; }
  tag(): number { return 9; }
  count(n: number): string {
    const out: (number | null)[] = [];
    for (let i = 0; i < n; i++) out.push(this.at(i));
    return out.join(",");
  }
}
const b = new Board();
delete (b.board as any)[5];
console.log(b.count(10));
// K7: an argument with an effect reassigns the field first.
console.log(b.at(b.swap()), b.board.length);
// K1: an override at run time.
class Sub extends Board { at(i: number): number { return 1000 + i; } }
const pick: Board = process.argv.length > 0 ? new Sub() : new Board();
let s = 0;
for (let i = 0; i < 4; i++) s += pick.at(i) as number;
console.log(s);
// K14: a side-effecting argument beside the index.
class Two {
  board: number[] = [5, 6, 7];
  log: string[] = [];
  at2(i: number, t: number): number { return this.board[i] ?? 0; }
  mark(x: number): number { this.log.push("m" + x); return x; }
}
const t = new Two();
let u = 0;
for (let i = 0; i < 3; i++) u += t.at2(i, t.mark(i));
console.log(u, t.log.join(""));
