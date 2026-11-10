// Arrays held in class fields read through the array representation tier
// (DESIGN-p4l1 §3, §4.2, PR-C3): direct `this.f[i]` reads and reads behind
// a method (`this.at(i)` is `this.f[i] ?? 0`), inside and outside the
// floor, with the controls that must keep the bound, the hole test and the
// `??`. Every line is a value node prints too.

class Board {
  board: number[] = new Array(128).fill(0);
  side: number = 1;
  ep: number = -1;
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  set(sq: number, piece: number): void {
    const old = this.at(sq);
    this.board[sq] = piece + old * 0;
  }
  // Keys inside the floor at every call.
  sumRanks(): number {
    let s = 0;
    for (let r = 0; r < 8; r++) {
      for (let f = 0; f < 8; f++) s += this.at(r * 16 + f) * (r + 1);
    }
    return s;
  }
  // A masked key, and a direct read of the field.
  masked(k: number): number {
    return this.at(k & 127) + (this.board[k & 127] ?? 50) + this.board[(k + 1) & 127];
  }
  // Keys that leave the floor: the `??` is the value.
  around(sq: number): string {
    return [this.at(sq - 16), this.at(sq + 16), this.at(sq - 200), this.at(sq + 200)].join(",");
  }
  ray(from: number, step: number): number {
    let n = 0;
    let sq = from + step;
    while ((sq & 0x88) === 0) {
      if (this.at(sq) !== 0) return n * 10 + this.at(sq);
      n++;
      sq += step;
    }
    return -n - 1;
  }
}

const b = new Board();
for (let i = 0; i < 128; i++) if ((i & 0x88) === 0) b.set(i, (i * 7) % 13 - 6);
console.log("ranks", b.sumRanks());
let m = 0;
for (let i = -300; i < 300; i++) m += b.masked(i);
console.log("masked", m);
console.log("around", b.around(0), b.around(119), b.around(64));
console.log("ray", b.ray(0, 1), b.ray(4, 16), b.ray(119, -17), b.ray(7, 1));

// Fixture 11: the field is reassigned to an array with another floor. The
// facts join the two; reads in the same method see whichever is there.
class Swap {
  cells: number[] = new Array(32).fill(1);
  at(i: number): number {
    return this.cells[i] ?? -1;
  }
  total(n: number): number {
    let s = 0;
    for (let i = 0; i < n; i++) s += this.at(i & 31);
    return s;
  }
  shrink(): number {
    const before = this.total(64);
    this.cells = new Array(8).fill(2);
    return before * 1000 + this.total(64);
  }
}
const sw = new Swap();
console.log("swap", sw.total(40), sw.shrink(), sw.at(7), sw.at(8), sw.cells.length);

// K-FS3: a computed-key store names the field.
class Named {
  data: number[] = new Array(16).fill(3);
  at(i: number): number {
    return this.data[i] ?? -1;
  }
  replace(name: string, other: number[]): void {
    (this as any)[name] = other;
  }
  sum(): number {
    let s = 0;
    for (let i = 0; i < 16; i++) s += this.at(i);
    return s;
  }
}
const nm = new Named();
const firstSum = nm.sum();
nm.replace(process.argv.length > 100 ? "other" : "data", [9, 9]);
console.log("named", firstSum, nm.sum(), nm.at(1), nm.at(2));

// Negative controls. A stored `undefined` is a present element: `??`
// applies to it.
class Undef {
  v: (number | undefined)[] = new Array(8).fill(4);
  at(i: number): number {
    return this.v[i] ?? 77;
  }
  run(): string {
    const a = this.at(1);
    this.v[1] = undefined;
    return [a, this.at(1), this.at(2), typeof this.v[1]].join(",");
  }
}
console.log("undef", new Undef().run());

// A field that also holds `null`.
class Maybe {
  v: number[] | null = null;
  at(i: number): number {
    return this.v === null ? -5 : (this.v[i] ?? -6);
  }
  fill(): void {
    this.v = new Array(4).fill(8);
  }
}
const mb = new Maybe();
const beforeFill = mb.at(1);
mb.fill();
console.log("maybe", beforeFill, mb.at(1), mb.at(4));

// Holes: no fused fill, a sparse store past the end, `delete`, a length
// store.
class Holes {
  bare: number[] = new Array(8);
  sparse: number[] = new Array(8).fill(1);
  gone: number[] = new Array(8).fill(2);
  cut: number[] = new Array(8).fill(3);
  bareAt(i: number): number {
    return this.bare[i] ?? -1;
  }
  sparseAt(i: number): number {
    return this.sparse[i] ?? -2;
  }
  goneAt(i: number): number {
    return this.gone[i] ?? -3;
  }
  cutAt(i: number): number {
    return this.cut[i] ?? -4;
  }
  run(): string {
    const out: number[] = [];
    this.bare[2] = 5;
    for (let i = 0; i < 4; i++) out.push(this.bareAt(i));
    this.sparse[20] = 6;
    out.push(this.sparseAt(7), this.sparseAt(8), this.sparseAt(20), this.sparse.length);
    delete this.gone[3];
    out.push(this.goneAt(2), this.goneAt(3), this.goneAt(4));
    this.cut.length = 2;
    out.push(this.cutAt(1), this.cutAt(2), this.cutAt(7), this.cut.length);
    return out.join(",");
  }
}
console.log("holes", new Holes().run());

// A deleted field and a subclass that overrides the reader.
class Hard {
  v: number[] = new Array(4).fill(6);
  at(i: number): number {
    const a = this.v;
    return a === undefined ? -9 : (a[i] ?? -8);
  }
}
const hd = new Hard();
const beforeDelete = hd.at(1);
delete (hd as any).v;
console.log("hard", beforeDelete, hd.at(1));

class Base {
  v: number[] = new Array(4).fill(1);
  at(i: number): number {
    return this.v[i] ?? 0;
  }
}
class Derived extends Base {
  at(i: number): number {
    return 40 + (this.v[i] ?? 0);
  }
}
const objs: Base[] = [new Base(), new Derived()];
let poly = 0;
for (let i = 0; i < 8; i++) poly += objs[i & 1]!.at(i & 3);
console.log("poly", poly);

// Elements that are not Numbers behind the reader.
class Words {
  w: string[] = new Array(4).fill("a");
  at(i: number): string {
    return this.w[i] ?? "none";
  }
  get(i: number): string {
    return this.w[i];
  }
}
const ws = new Words();
ws.w[2] = "b";
console.log("words", ws.at(0), ws.at(2), ws.at(4), ws.get(1), ws.get(2), ws.get(9));

// A fraction, NaN and -0 as the key.
console.log("keys", b.at(0.5), b.at(NaN), b.at(-0), b.at(127), b.at(128));
