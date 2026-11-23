// N2 (DESIGN-n2-fn-bases.md): the element base of a field-held board held
// from the first read of a method body to its end. The positive shape is
// chess's `attacked`: for-of walks over constant arrays (unrolled by N3)
// and rays, every board read through `at`. Then the negative controls the
// region must refuse or re-derive across: a callee that writes the field,
// a callee that pushes onto the same board through an alias, a getter that
// grows the board, a callback that relocates the board, allocations (and
// so collections) between reads, a throw, and a key outside the floor.

const KNIGHT = [-33, -31, -18, -14, 14, 18, 31, 33];
const KING = [-17, -16, -15, -1, 1, 15, 16, 17];

function onBoard(sq: number): boolean {
  return sq >= 0 && sq < 128 && (sq & 0x88) === 0;
}

let shared: number[] = [];

class Board {
  board: number[] = new Array(128).fill(0);
  spare: number[] = new Array(128).fill(7);
  hits = 0;
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  get grower(): number {
    for (let k = 0; k < 200; k++) this.board.push(k & 3);
    return 1;
  }
  // Positive: the attacked shape.
  attacked(square: number, color: number): boolean {
    for (const delta of KNIGHT) {
      const source = square + delta;
      if (onBoard(source) && this.at(source) === color * 2) return true;
    }
    for (const delta of KING) {
      let source = square + delta;
      let distance = 1;
      while (onBoard(source)) {
        const piece = this.at(source);
        if (piece !== 0) {
          if (piece * color > 0) {
            const type = Math.abs(piece);
            const diagonal = delta === -17 || delta === -15 || delta === 15 || delta === 17;
            if (type === 5 || (diagonal && type === 3) || (!diagonal && type === 4) || (distance === 1 && type === 6)) return true;
          }
          break;
        }
        source += delta;
        distance++;
      }
    }
    return false;
  }
  // Straight-line reads, nothing between them.
  sum4(a: number, b: number): number {
    return this.at(a) + this.at(b) + this.at(a + 1) + this.at(b + 1);
  }
  swapIn(): void {
    const t = this.board;
    this.board = this.spare;
    this.spare = t;
  }
  growAlias(): void {
    const a = this.board;
    for (let k = 0; k < 300; k++) a.push(k & 1);
  }
  // A callee writes the field between reads.
  writes(): number {
    let s = this.at(1) + this.at(2);
    this.swapIn();
    s += this.at(1) + this.at(2);
    this.swapIn();
    return s;
  }
  // A callee pushes onto the board through an alias between reads.
  pushes(): number {
    let s = this.at(3) + this.at(4);
    this.growAlias();
    s += this.at(3) + this.at(300);
    return s;
  }
  // A getter grows the board between reads.
  getter(): number {
    let s = this.at(5) + this.at(6);
    s += this.grower;
    s += this.at(5) + this.at(6);
    return s;
  }
  // A callback relocates the board between reads.
  callback(): number {
    let s = this.at(7) + this.at(8);
    [1, 2, 3].forEach(() => {
      for (let k = 0; k < 100; k++) shared.push(k);
    });
    s += this.at(7) + this.at(8);
    return s;
  }
  // Allocations between reads.
  allocs(): number {
    let s = this.at(9);
    let junk: number[][] = [];
    for (let k = 0; k < 2000; k++) junk.push([k, k + 1]);
    s += this.at(9) + this.at(10) + junk.length;
    return s;
  }
  // A throw between reads.
  throws(sq: number): number {
    let s = this.at(sq);
    try {
      if (sq > 100) throw new Error("far");
      s += this.at(sq + 1);
    } catch (e) {
      s += 1000;
    }
    return s + this.at(sq);
  }
  // A key outside the floor in the held suffix.
  outside(sq: number): number {
    return this.at(sq) + this.at(sq + 200) + this.at(sq * 2);
  }
}

const b = new Board();
for (let i = 0; i < 128; i++) b.board[i] = (i * 7) % 13 - 6;
const blocker = new Board();
let att = 0;
for (let r = 0; r < 200; r++) {
  for (let sq = 0; sq < 128; sq++) {
    if ((sq & 0x88) !== 0) continue;
    if (b.attacked(sq, 1)) att++;
    if (b.attacked(sq, -1)) att += 2;
  }
}
console.log("attacked", att);
let s = 0;
for (let i = 0; i < 100; i++) s += b.sum4(i & 63, (i * 3) & 63);
console.log("sum4", s);
console.log("writes", b.writes(), b.writes());
console.log("pushes", b.pushes(), b.board.length);
console.log("getter", b.getter(), b.board.length);
shared = b.board;
console.log("callback", b.callback(), b.board.length);
console.log("allocs", b.allocs());
console.log("throws", b.throws(5), b.throws(120));
console.log("outside", b.outside(3), b.outside(100), blocker.at(3));
