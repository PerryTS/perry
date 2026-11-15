// P2, an index clone's parameter covered by a signed I32 signature slot:
// `put`'s parameters feed element keys, and its callers pass negative values
// for both, so their I32 intervals start below 0. The method is typed all
// the same (an int32 key is bounded by one unsigned compare). Under
// PERRY_FACTS_SABOTAGE=signature-range the facts claim [0, 127] for both,
// which would let the typed body drop that sign: the signature's interval
// witness must report the negative argument.
// A loop bound the compiler cannot fold keeps each call site one HIR call.
const RUN = Math.random() < 2 ? 1 : 0;
class Grid {
  cells: number[] = new Array<number>(16).fill(0);
  keys: number[] = [];
  h = 0;
  constructor() {
    for (let i = 0; i < 16 * 16; i++) this.keys.push((i * 2654435761) | 0);
  }
  put(square: number, piece: number): void {
    const old = this.cells[square] ?? 0;
    if (old !== 0) this.h ^= this.keys[(old + 6) * 16 + square] ?? 0;
    this.cells[square] = piece;
    if (piece !== 0) this.h ^= this.keys[(piece + 6) * 16 + square] ?? 0;
  }
}
const g = new Grid();
for (let i = 0; i < 16 * RUN; i++) g.put(i, (i % 9) - 4);
g.put(-2, -3);
console.log(g.h, g.cells.join(","), (g.cells as any)[-2]);
