// `this.m(i)` inlined into a hoisted loop when `m` is `return <pure read>`;
// the copy is guarded by `this.m === C.prototype.m`, so every override path
// must still call the override.
class B {
  board: number[] = [];
  constructor() { for (let i = 0; i < 128; i++) this.board.push((i * 5) % 11 - 5); }
  at(sq: number): number { return this.board[sq] ?? 0; }
  twice(sq: number): number { return (this.board[sq] ?? 0) * 2; }
  sum(n: number): number { let s = 0; for (let i = 0; i < n; i++) s += this.at(i & 127); return s; }
  sum2(n: number): number { let s = 0; for (let i = 0; i < n; i++) s += this.twice(i & 127) + this.at(i & 63); return s; }
  holes(n: number): number { let s = 0; for (let i = 0; i < n; i++) s += this.at(i); return s; }
}
class Sub extends B { at(sq: number): number { return 100; } }
let calls = 0;
const b = new B();
console.log(b.sum(1000), b.sum2(1000), b.holes(300));
console.log(new Sub().sum(10));
const own: any = new B();
own.at = (sq: number) => { calls++; return 1; };
console.log(own.sum(10), calls);
const patched = new B();
const orig = B.prototype.at;
B.prototype.at = function (sq: number) { calls++; return -1; };
console.log(patched.sum(5), calls);
B.prototype.at = orig;
console.log(patched.sum(5));
