// `this.F[...]` reads of a declared array field hoisted out of a loop in an
// instance method. Every way the field could change or be observed during the
// loop must behave as if it were read on every iteration.
class Box {
  board: number[] = [];
  zob: number[] = [];
  hash = 0;
  rights = 15;
  constructor() {
    for (let i = 0; i < 128; i++) this.board.push(((i * 7) % 13) - 6);
    let seed = 0x6d2b79f5;
    for (let i = 0; i < 1024; i++) { seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; this.zob.push(seed); }
  }
  direct(n: number): number { let s = 0; for (let i = 0; i < n; i++) s += this.board[i & 127] as number; return s; }
  withStores(n: number): number {
    for (let i = 0; i < n; i++) { this.hash ^= this.zob[i & 1023] ?? 0; this.rights &= ~(i & 3); this.rights |= i & 15; }
    return this.hash + this.rights;
  }
  holes(n: number): number { let s = 0; for (let i = 0; i < n; i++) s += this.board[i] ?? -1000; return s; }
  // The loop replaces the array: every later read must see the new one.
  reassigning(n: number): number {
    let s = 0;
    for (let i = 0; i < n; i++) { s += this.board[i & 127] as number; if (i === 50) this.board = this.board.map((v) => v * 10); }
    return s;
  }
  // A call in the loop could replace the array, so nothing is hoisted.
  swap(): void { this.board = this.board.slice().reverse(); }
  calling(n: number): number { let s = 0; for (let i = 0; i < n; i++) { s += (this.board[i & 127] as number) * (i + 1); if ((i & 63) === 63) this.swap(); } return s; }
  // Element writes keep the array identity.
  writing(n: number): number { for (let i = 0; i < n; i++) this.board[i & 127] = (this.board[(i + 1) & 127] as number) + 1; return this.direct(128); }
  nested(n: number): number { let s = 0; for (let j = 0; j < n; j++) for (let i = 0; i < 128; i++) s += (this.board[i] as number) * j; return s; }
  zeroTrip(): number { let s = 0; for (let i = 0; i < 0; i++) s += this.board[i] as number; return s; }
}
class Sub extends Box {
  // A subclass accessor does not reach the base field: the field is an own
  // data property.
  get extra(): number { return 1; }
}
const b = new Box();
console.log(b.direct(1000), b.withStores(5000), b.holes(200));
console.log(new Box().reassigning(200), new Box().calling(1000), new Box().writing(1000), new Box().nested(5));
console.log(new Sub().direct(300));
// A detached call: `this` is undefined, a loop that never runs reads nothing.
const zeroTrip = Box.prototype.zeroTrip;
console.log(zeroTrip.call(undefined));
// A foreign receiver whose `board` is a getter: the original per-iteration
// reads must happen (the hoist is versioned on `this instanceof Box`).
let reads = 0;
const foreign: any = { get board() { reads++; return [1, 2, 3, 4]; }, zob: [], hash: 0, rights: 0 };
console.log(Box.prototype.direct.call(foreign, 10), reads);
try { Box.prototype.direct.call(undefined, 1); } catch (e) { console.log("threw", (e as Error).constructor.name); }
