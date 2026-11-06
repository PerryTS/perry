// Class-field loops with allocation inside the loop body, so moving
// collections run between field accesses of the same iteration. A root
// reload reused across one of those collections would read or write the
// object's old copy; the checksums below would diverge.
// `gc()` (Perry always; Node only with --expose-gc) forces a collection
// between two accesses of the same field in one iteration.
declare function gc(): void;
function maybeCollect(i: number): void { if ((i & 2047) === 0 && typeof gc === "function") gc(); }
type Color = "white" | "black";
function sign(c: Color): number { return c === "white" ? 1 : -1; }
class G {
  hash = 0;
  rights = 15;
  side: Color = "white";
  zob: number[] = [];
  board: number[] = [];
  keep: any[] = [];
  constructor() {
    let seed = 0x6d2b79f5;
    for (let i = 0; i < 1024; i++) { seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; this.zob.push(seed); }
    for (let i = 0; i < 128; i++) this.board.push(((i * 7) % 13) - 6);
  }
  run(n: number): any[] {
    let s = 0;
    for (let i = 0; i < n; i++) {
      this.hash ^= this.zob[i & 1023] ?? 0;
      const junk = { a: i, b: [i, i + 1, i + 2], c: "x" + i };
      this.rights &= ~(i & 3);
      maybeCollect(i);
      const more = [junk, { d: junk.c + "y" }];
      this.rights |= i & 15;
      if ((i & 255) === 0) this.keep.push(more);
      s += (junk.b[2] as number) - junk.a + sign(this.side);
      this.side = this.side === "white" ? "black" : "white";
      s += this.board[i & 127] as number;
    }
    return [this.hash, this.rights, s, this.keep.length, this.side];
  }
  // Call-free loops over a freshly allocated (young) array field: the packed
  // tier keeps the element base and `this` in registers between strided
  // polls, and a collection at one of those polls moves both.
  dense(rounds: number): number {
    let total = 0;
    for (let r = 0; r < rounds; r++) {
      const fresh: number[] = [];
      for (let k = 0; k < 1024; k++) fresh.push((k * 31 + r) % 97);
      this.zob = fresh;
      this.board = fresh.slice(0, 128);
      for (let i = 0; i < 5000; i++) { this.hash ^= this.zob[i & 1023] ?? 0; this.rights &= ~(i & 3); this.rights |= i & 15; }
      let s = 0;
      for (let i = 0; i < 5000; i++) s += this.at(i & 127);
      total += s + this.hash + this.rights;
    }
    return total;
  }
  at(sq: number): number { return this.board[sq] ?? 0; }
  // Tight non-allocating loops between allocating phases: each tight loop's
  // reuse must restart after the collections the previous phase triggered.
  phases(rounds: number): number {
    let total = 0;
    for (let r = 0; r < rounds; r++) {
      const garbage: number[][] = [];
      for (let k = 0; k < 2000; k++) garbage.push([k, r, k ^ r]);
      for (let i = 0; i < 3000; i++) { this.hash += i & 7; this.rights ^= i; }
      if (typeof gc === "function") gc();
      for (let i = 0; i < 3000; i++) { this.rights ^= i; this.hash -= i & 3; maybeCollect(i); this.hash += this.zob[i & 1023]! & 1; }
      total += garbage[r % 2000]![2]! + this.hash + this.rights;
    }
    return total;
  }
}
const gs: G[] = [];
for (let k = 0; k < 4; k++) gs.push(new G());
for (const g of gs) console.log(JSON.stringify(g.run(20000)));
console.log(gs[0]!.phases(40));
console.log(gs[1]!.dense(30));
let sum = 0;
for (const g of gs) sum += g.keep.length + g.keep[3][1].d.length;
console.log(sum);
