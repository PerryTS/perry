// A board whose cells hold `color * kind`, `-0` for kind 0 and color -1. A
// cell is read through `?? 0` into `x`, which an int32 compare and `1 / x`
// use. The division observes the sign, so the cells keep their double `-0`
// and `1 / x` is -Infinity (−0 tolerance, PR D). Under
// PERRY_FACTS_SABOTAGE=zerosign the division is no sign-observing use: the
// cells and `x` are I32 by tolerance and `1 / x` would be Infinity. The sign
// witness at the division must report the false claim.
class Board {
  cells: number[] = new Array(8).fill(0);
  put(i: number, color: number, kind: number): void {
    this.cells[i & 7] = color * kind;
  }
  at(i: number): number {
    return this.cells[i & 7] ?? 0;
  }
}
const b = new Board();
const color = process.argv.length > 100 ? 1 : -1;
for (let i = 0; i < 8; i++) b.put(i, color, i & 3);
let out = "";
for (let i = 0; i < 8; i++) {
  const x = b.at(i);
  out += x === 0 ? String(1 / x) : String(x & 7);
  out += " ";
}
console.log(out);
