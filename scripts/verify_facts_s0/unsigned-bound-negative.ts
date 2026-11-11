// N10: `at` reads `this.board[k | 0]` with no sign test in front of the
// bound. `idx <u capacity` declines a negative key, and the tier's miss arm
// sends it to the exact property-key read before the cold element read.
// Under PERRY_FACTS_SABOTAGE=unsignedbound the miss arm drops that exit, so
// a negative key reaches the element tier's cold read; its check-mode
// witness must report it ("unsigned bound").
class Board {
  board: number[] = new Array(16).fill(1);
  ep: number = process.argv.length > 100 ? 5 : 3;
  at(sq: number): number {
    // No `??`: over an Array of Numbers that is the rep-keyed default read
    // (`index_get::rep_default`), whose miss arm always reads the exact key.
    const v = this.board[sq | 0];
    return v === undefined ? 0 : v;
  }
  probe(): number {
    const ep = this.ep;
    return this.at(ep) * 100 + this.at(ep - 16) * 10 + this.at(ep + 16);
  }
}
const b = new Board();
let s = 0;
for (let i = 0; i < 3; i++) s += b.probe();
const plain: number[] = [4, 5, 6];
const k = process.argv.length > 100 ? 1 : -2;
const pk = plain[k | 0];
console.log(s, pk === undefined ? 7 : pk);
