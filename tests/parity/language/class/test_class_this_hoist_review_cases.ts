// Review cases for the `this.F` loop hoist and its helpers (#14182): each
// must behave exactly as the unoptimized loop does.
class B {
  board: number[] = [1, 2, 3, 4, 5, 6, 7, 8];
  at(sq: number): number { return this.board[sq] ?? 0; }
  first(sq: number): number { return this.board[0] ?? 0; }
  plusTwice(sq: number): number { return (this.board[sq & 7] ?? 0) + sq + sq; }
  sum(n: number): number { let s = 0; for (let i = 0; i < n; i++) s += this.at(i & 7); return s; }
  sameAt(): boolean { return this.at === B.prototype.at; }
  notSameAt(): boolean { return this.at !== B.prototype.at; }
  // A getter on another receiver replaces the field mid-loop.
  viaState(n: number, st: any): number { let s = 0; for (let i = 0; i < n; i++) { s += this.board[i & 7] as number; s += st.next; } return s; }
  // A TDZ read in an argument the inlined body never uses.
  tdz(): string {
    const read = (): number => late;
    try { let s = 0; for (let i = 0; i < 2; i++) s += this.first(read()); return "no throw " + s; }
    catch (e) { return (e as Error).constructor.name; }
    finally { /* late is declared below */ }
    let late = 1;
  }
  // An argument read twice by the inlined body, changed by valueOf between.
  twice(n: number): number { let x: any = 1; let s = 0; for (let i = 0; i < n; i++) s += this.plusTwice(x); return s; }
  zeroTrip(): number { let s = 0; for (let i = 0; i < 0; i++) s += this.board[i] as number; return s; }
}
const b = new B();
console.log(b.sum(16), b.sameAt(), b.notSameAt());
const orig = B.prototype.at;
B.prototype.at = function (sq: number) { return 100; };
console.log(new B().sum(4), new B().sameAt(), new B().notSameAt());
B.prototype.at = orig;
let replaced = 0;
const holder = new B();
const st = { get next() { if (++replaced === 3) holder.board = [10, 10, 10, 10, 10, 10, 10, 10]; return 0; } };
console.log(holder.viaState(8, st));
console.log(new B().tdz());
console.log(new B().twice(3));
// An accessor redefined on an instance.
const acc = new B();
let gets = 0;
Object.defineProperty(acc, "board", { get() { gets++; return [2, 2, 2, 2, 2, 2, 2, 2]; } });
console.log(acc.sum(5), gets);
// A Symbol.hasInstance hook is never consulted by the loop.
let hooks = 0;
Object.defineProperty(B, Symbol.hasInstance, { value: () => { hooks++; return false; } });
console.log(new B().zeroTrip(), new B().sum(2), hooks);
// An integer-returning helper with a bare `return;` path.
function maybe(c: boolean): number { if (c) return; return c ? 1 : -1; }
let acc2 = 0;
for (let i = 0; i < 4; i++) { const v: any = maybe(i === 2); acc2 = acc2 + (v === undefined ? 1000 : v); }
console.log(acc2, String(maybe(true)));
